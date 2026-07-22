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

#[derive(Debug, Clone)]
pub(super) struct PendingCreatureAppearanceTailRepair {
    original_bits: Vec<bool>,
    original_tail_start: usize,
    rewritten_tail_start: usize,
    inserted_bits: usize,
    removed_bits: usize,
    appearance_offset: usize,
    object_id: u32,
}

impl PendingCreatureAppearanceTailRepair {
    pub(super) fn new(
        original_bits: Vec<bool>,
        original_tail_start: usize,
        rewritten_tail_start: usize,
        inserted_bits: usize,
        removed_bits: usize,
        appearance_offset: usize,
        object_id: u32,
    ) -> Option<Self> {
        if original_tail_start > original_bits.len() {
            return None;
        }
        Some(Self {
            original_bits,
            original_tail_start,
            rewritten_tail_start,
            inserted_bits,
            removed_bits,
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
    pub bytes_inserted: usize,
    pub bytes_removed: usize,
    pub bits_inserted: usize,
}

pub(super) fn try_repair_for_creature_update(
    pending: &PendingCreatureAppearanceTailRepair,
    live_bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
    fragment_bits: &mut Vec<bool>,
    bit_cursor: usize,
) -> Option<CreatureTailRepairResult> {
    if pending.rewritten_tail_start > fragment_bits.len() || bit_cursor > fragment_bits.len() {
        return None;
    }
    if live_object_record_object_id(live_bytes, offset, *record_end) != Some(pending.object_id) {
        trace_tail_repair_reject(pending, offset, *record_end, bit_cursor);
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

        trace_tail_repair_candidate(
            pending,
            offset,
            *record_end,
            candidate_tail_start,
            bit_cursor,
            candidate_bits.len(),
        );
        let mut trial_cursor = bit_cursor;
        if creature::advance_verified_noop_creature_update_record(
            live_bytes,
            offset,
            *record_end,
            &candidate_bits,
            &mut trial_cursor,
        ) {
            let old_bits_len = fragment_bits.len();
            let new_bits_len = candidate_bits.len();
            *fragment_bits = candidate_bits;
            trace_tail_repair_accept(
                pending,
                offset,
                *record_end,
                candidate_tail_start,
                bit_cursor,
                trial_cursor,
                old_bits_len,
                new_bits_len,
                0,
                0,
                0,
            );
            return Some(CreatureTailRepairResult {
                bit_cursor: trial_cursor,
                old_bits_len,
                new_bits_len,
                bytes_inserted: 0,
                bytes_removed: 0,
                bits_inserted: 0,
            });
        }

        let mut trial_bytes = live_bytes.clone();
        let mut trial_bits = candidate_bits.clone();
        let mut trial_record_end = *record_end;
        if let Some(action0_rewrite) = creature::remove_3967_action0_legacy_bridge_followup_for_ee(
            &mut trial_bytes,
            offset,
            &mut trial_record_end,
            &mut trial_bits,
            bit_cursor,
        ) {
            let mut trial_cursor = bit_cursor;
            if creature::advance_verified_noop_creature_update_record(
                &trial_bytes,
                offset,
                trial_record_end,
                &trial_bits,
                &mut trial_cursor,
            ) {
                let old_bits_len = fragment_bits.len();
                let new_bits_len = trial_bits.len();
                *live_bytes = trial_bytes;
                *record_end = trial_record_end;
                *fragment_bits = trial_bits;
                trace_tail_repair_accept(
                    pending,
                    offset,
                    *record_end,
                    candidate_tail_start,
                    bit_cursor,
                    trial_cursor,
                    old_bits_len,
                    new_bits_len,
                    action0_rewrite.bytes_inserted,
                    action0_rewrite.bytes_removed,
                    action0_rewrite.bits_inserted,
                );
                return Some(CreatureTailRepairResult {
                    bit_cursor: trial_cursor,
                    old_bits_len,
                    new_bits_len,
                    bytes_inserted: action0_rewrite.bytes_inserted,
                    bytes_removed: action0_rewrite.bytes_removed,
                    bits_inserted: action0_rewrite.bits_inserted,
                });
            }
        }

        let mut trial_bytes = live_bytes.clone();
        let mut trial_record_end = *record_end;
        if let Some(action0_damage_rewrite) =
            creature::insert_3967_action0_missing_damage_byte_for_ee(
                &mut trial_bytes,
                offset,
                &mut trial_record_end,
                &candidate_bits,
                bit_cursor,
            )
        {
            let mut trial_cursor = bit_cursor;
            if creature::advance_verified_noop_creature_update_record(
                &trial_bytes,
                offset,
                trial_record_end,
                &candidate_bits,
                &mut trial_cursor,
            ) {
                let old_bits_len = fragment_bits.len();
                let new_bits_len = candidate_bits.len();
                *live_bytes = trial_bytes;
                *record_end = trial_record_end;
                *fragment_bits = candidate_bits;
                trace_tail_repair_accept(
                    pending,
                    offset,
                    *record_end,
                    candidate_tail_start,
                    bit_cursor,
                    trial_cursor,
                    old_bits_len,
                    new_bits_len,
                    action0_damage_rewrite.bytes_inserted,
                    0,
                    0,
                );
                return Some(CreatureTailRepairResult {
                    bit_cursor: trial_cursor,
                    old_bits_len,
                    new_bits_len,
                    bytes_inserted: action0_damage_rewrite.bytes_inserted,
                    bytes_removed: 0,
                    bits_inserted: 0,
                });
            }
        }

        let mut trial_bytes = live_bytes.clone();
        let mut trial_record_end = *record_end;
        if let Some(action0_associate_rewrite) =
            creature::insert_3967_action0_short_associate_suffix_for_ee(
                &mut trial_bytes,
                offset,
                &mut trial_record_end,
                &candidate_bits,
                bit_cursor,
            )
        {
            let mut trial_cursor = bit_cursor;
            if creature::advance_verified_noop_creature_update_record(
                &trial_bytes,
                offset,
                trial_record_end,
                &candidate_bits,
                &mut trial_cursor,
            ) {
                let old_bits_len = fragment_bits.len();
                let new_bits_len = candidate_bits.len();
                *live_bytes = trial_bytes;
                *record_end = trial_record_end;
                *fragment_bits = candidate_bits;
                trace_tail_repair_accept(
                    pending,
                    offset,
                    *record_end,
                    candidate_tail_start,
                    bit_cursor,
                    trial_cursor,
                    old_bits_len,
                    new_bits_len,
                    action0_associate_rewrite.bytes_inserted,
                    0,
                    0,
                );
                return Some(CreatureTailRepairResult {
                    bit_cursor: trial_cursor,
                    old_bits_len,
                    new_bits_len,
                    bytes_inserted: action0_associate_rewrite.bytes_inserted,
                    bytes_removed: 0,
                    bits_inserted: 0,
                });
            }
        }
    }

    trace_tail_repair_reject(pending, offset, *record_end, bit_cursor);
    None
}

fn trace_tail_repair_candidate(
    pending: &PendingCreatureAppearanceTailRepair,
    offset: usize,
    record_end: usize,
    candidate_tail_start: usize,
    bit_cursor: usize,
    candidate_bits_len: usize,
) {
    if !crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    ) {
        return;
    }
    eprintln!(
        "live-object creature-P tail repair candidate: p_offset={} object_id=0x{:08X} record_offset={offset} record_end={record_end} original_tail={} candidate_tail={} rewritten_tail={} inserted_bits={} bit_cursor={bit_cursor} candidate_bits_len={candidate_bits_len}",
        pending.appearance_offset,
        pending.object_id,
        pending.original_tail_start,
        candidate_tail_start,
        pending.rewritten_tail_start,
        pending.inserted_bits,
    );
}

fn candidate_tail_starts(pending: &PendingCreatureAppearanceTailRepair) -> Vec<usize> {
    // This is a splice, not a cursor resynchronizer. The caller computes the
    // original tail start from the exact appearance bit delta; if that start is
    // wrong, the following focused creature-update validator must reject.
    vec![pending.original_tail_start]
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
    bytes_inserted: usize,
    bytes_removed: usize,
    bits_inserted: usize,
) {
    if !crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    ) {
        return;
    }
    eprintln!(
        "live-object creature-P tail repaired: p_offset={} object_id=0x{:08X} record_offset={offset} record_end={record_end} original_tail={} accepted_tail={} rewritten_tail={} inserted_bits={} removed_bits={} bit_cursor={bit_cursor}->{repaired_cursor} bits_len={old_bits_len}->{new_bits_len} action0_bytes_inserted={bytes_inserted} action0_bytes_removed={bytes_removed} action0_bits_inserted={bits_inserted}",
        pending.appearance_offset,
        pending.object_id,
        pending.original_tail_start,
        candidate_tail_start,
        pending.rewritten_tail_start,
        pending.inserted_bits,
        pending.removed_bits,
    );
}

fn trace_tail_repair_reject(
    pending: &PendingCreatureAppearanceTailRepair,
    offset: usize,
    record_end: usize,
    bit_cursor: usize,
) {
    if !crate::translate::live_object_update::live_object_debug_env_enabled(
        "HGBRIDGE_PROXY2_DEBUG_LIVE_CLAIM",
    ) {
        return;
    }
    eprintln!(
        "live-object creature-P tail repair rejected: p_offset={} object_id=0x{:08X} record_offset={offset} record_end={record_end} original_tail={} rewritten_tail={} inserted_bits={} removed_bits={} bit_cursor={bit_cursor}",
        pending.appearance_offset,
        pending.object_id,
        pending.original_tail_start,
        pending.rewritten_tail_start,
        pending.inserted_bits,
        pending.removed_bits,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_repair_candidates_do_not_scan_neighboring_cursors() {
        let pending =
            PendingCreatureAppearanceTailRepair::new(vec![false; 32], 10, 12, 2, 0, 0, 0x8000_0001)
                .expect("test pending repair");

        assert_eq!(
            candidate_tail_starts(&pending),
            vec![10],
            "appearance tail repair must splice the decompile-owned source tail, not search nearby bit fits"
        );
    }

    #[test]
    fn tail_repair_candidates_remain_exact_when_no_bits_were_inserted() {
        let pending =
            PendingCreatureAppearanceTailRepair::new(vec![false; 32], 10, 10, 0, 0, 0, 0x8000_0001)
                .expect("test pending repair");

        assert_eq!(
            candidate_tail_starts(&pending),
            vec![10],
            "already-EE-shaped appearance rows must not enable a +/- cursor window"
        );
    }
}
