use super::*;

// Split candidate selection and visibly heuristic quickbar boundary scoring.
//
// EE/Diamond quickbar SetAllButtons writes 36 slot records plus a compact BOOL
// fragment tail. HG captures that are semantically claimable have used tiny tail
// sizes (0, 12, 28, 36 bytes). Keeping this scan bounded is important because
// each candidate tail invokes the item-boundary scorer; a broad 512-byte sweep
// is both heuristic pressure and slow enough to trigger server retransmit loops.
const MAX_DECOMPILE_OWNED_QUICKBAR_FRAGMENT_TAIL_SCAN_BYTES: usize = 64;

#[derive(Debug, Clone, Copy)]
pub(super) struct QuickbarTransportSplit {
    pub(super) read_body_len: usize,
    pub(super) fragment_tail_len: usize,
    pub(super) translated_item_slots: u32,
    pub(super) spell_slots: u32,
    pub(super) general_slots: u32,
    pub(super) item_candidate_slots: u32,
    pub(super) unsupported_slots: u32,
    pub(super) trailing_read_bytes: usize,
}

impl QuickbarTransportSplit {
    fn preserves_decompile_owned_payload(&self) -> bool {
        self.translated_item_slots != 0
            || self.spell_slots != 0
            || self.general_slots != 0
            || self.item_candidate_slots != 0
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) enum QuickbarSplitPolicy {
    /// Decompile-backed quickbar transport repair for server-to-client
    /// `GuiQuickbar_SetAllButtons`: EE/Diamond both read exactly 36 slot type
    /// bytes and the item/spell payloads that follow them. A split is allowed
    /// only when the semantic translator can preserve real item or spell slots,
    /// consumes the read buffer exactly, and does not rely on candidate/unknown
    /// blanking to make the parse fit.
    ExactSemantic,

    /// Decompile-backed boundary ownership for the live HG all-buttons stream.
    /// The EE and Diamond handlers both consume a 36-slot quickbar record. When
    /// our item parser does not yet own one legacy item-object body, the reader
    /// may still prove the next slot boundary using the bounded scorer and the
    /// writer emits an empty EE slot for that single `ItemCandidate` source
    /// span. This is still strict translation: no raw candidate bytes are
    /// forwarded, and generic `Unsupported`/tail-blanking may not claim the
    /// packet because that would mask an incomplete stream.
    DecompileOwnedBoundary,
}

pub(super) fn choose_quickbar_split(
    body_and_tail: &[u8],
    prefixed_fragment_bytes: &[u8],
    policy: QuickbarSplitPolicy,
) -> Option<QuickbarTransportSplit> {
    if body_and_tail.len() < LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }

    let max_tail = body_and_tail
        .len()
        .saturating_sub(LEGACY_QUICKBAR_BUTTON_COUNT)
        .min(MAX_QUICKBAR_FOUR_PREFIX_FRAGMENT_TAIL_BYTES)
        .min(MAX_DECOMPILE_OWNED_QUICKBAR_FRAGMENT_TAIL_SCAN_BYTES);
    let mut best: Option<QuickbarTransportSplit> = None;
    let mut best_score = i32::MIN;
    let mut best_rejected: Option<(QuickbarTransportSplit, i32, &'static str)> = None;
    let mut best_rejected_score = i32::MIN;

    for fragment_tail_len in 0..=max_tail {
        let read_body_len = body_and_tail.len().checked_sub(fragment_tail_len)?;
        let mut read_buffer = Vec::with_capacity(read_body_len.checked_add(CNW_LENGTH_BYTES)?);
        read_buffer.extend_from_slice(&[0, 0, 0, 0]);
        read_buffer.extend_from_slice(body_and_tail.get(..read_body_len)?);

        let mut fragments = Vec::with_capacity(
            prefixed_fragment_bytes
                .len()
                .checked_add(fragment_tail_len)?,
        );
        fragments.extend_from_slice(prefixed_fragment_bytes);
        fragments.extend_from_slice(body_and_tail.get(read_body_len..)?);

        let Some((buttons, final_cursor)) = parse_quickbar_read_buffer_with_fragments(
            &read_buffer,
            &fragments,
            LEGACY_QUICKBAR_READ_CURSOR_START,
        ) else {
            continue;
        };
        let translated_item_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
            .count() as u32;
        let spell_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
            .count() as u32;
        let general_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::General { .. }))
            .count() as u32;
        let item_candidate_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::ItemCandidate))
            .count() as u32;
        let unsupported_slots = buttons
            .iter()
            .filter(|button| matches!(button.kind, QuickbarButtonKind::Unsupported))
            .count() as u32;
        let blanked_or_unsupported = buttons
            .iter()
            .filter(|button| {
                matches!(
                    button.kind,
                    QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported
                )
            })
            .count() as i32;
        let trailing_read_bytes = read_buffer.len().saturating_sub(final_cursor);
        let split = QuickbarTransportSplit {
            read_body_len,
            fragment_tail_len,
            translated_item_slots,
            spell_slots,
            general_slots,
            item_candidate_slots,
            unsupported_slots,
            trailing_read_bytes,
        };
        let score = translated_item_slots as i32 * 200
            + spell_slots as i32 * 120
            + general_slots as i32 * 8
            - blanked_or_unsupported * 100
            - trailing_read_bytes.min(512) as i32
            - fragment_tail_len.min(128) as i32;
        let reject_reason = match policy {
            QuickbarSplitPolicy::ExactSemantic => {
                if translated_item_slots == 0 && spell_slots == 0 {
                    Some("exact-no-item-or-spell")
                } else if item_candidate_slots != 0 || unsupported_slots != 0 {
                    Some("exact-blanked-or-unsupported")
                } else if trailing_read_bytes != 0 {
                    Some("exact-trailing-read-bytes")
                } else {
                    None
                }
            }
            QuickbarSplitPolicy::DecompileOwnedBoundary => {
                if translated_item_slots == 0 && spell_slots == 0 {
                    // General/no-payload buttons and item-candidate blanks are
                    // too easy to manufacture from a shifted read cursor.  The
                    // Diamond and EE decompiles both define real quickbar
                    // semantics around owned item and spell records, so this
                    // fallback may only claim a split that preserves at least
                    // one decompile-owned item or spell slot. This prevents a
                    // whole quickbar from collapsing into 36 blank EE slots.
                    Some("boundary-no-item-or-spell")
                } else if unsupported_slots != 0 {
                    Some("boundary-unsupported-slots")
                } else if trailing_read_bytes != 0 {
                    Some("boundary-trailing-read-bytes")
                } else {
                    None
                }
            }
        };
        if let Some(reason) = reject_reason {
            if score > best_rejected_score {
                best_rejected_score = score;
                best_rejected = Some((split, score, reason));
            }
            continue;
        }

        if final_cursor > read_buffer.len() {
            continue;
        }

        if score > best_score {
            best_score = score;
            best = Some(split);
        }
    }

    if best.is_none() {
        best = choose_cursor_derived_quickbar_split(body_and_tail, prefixed_fragment_bytes, policy);
    }

    if best.is_none() {
        if let Some((split, score, reason)) = best_rejected {
            tracing::info!(
                policy = ?policy,
                body_and_tail_len = body_and_tail.len(),
                max_tail,
                read_body_len = split.read_body_len,
                fragment_tail_len = split.fragment_tail_len,
                translated_item_slots = split.translated_item_slots,
                spell_slots = split.spell_slots,
                general_slots = split.general_slots,
                item_candidate_slots = split.item_candidate_slots,
                unsupported_slots = split.unsupported_slots,
                trailing_read_bytes = split.trailing_read_bytes,
                score,
                reason,
                "server GuiQuickbar_SetAllButtons split parser found only rejected candidates"
            );
        }
    }

    best
}

fn choose_cursor_derived_quickbar_split(
    body_and_tail: &[u8],
    prefixed_fragment_bytes: &[u8],
    policy: QuickbarSplitPolicy,
) -> Option<QuickbarTransportSplit> {
    let mut read_buffer = Vec::with_capacity(body_and_tail.len().checked_add(CNW_LENGTH_BYTES)?);
    read_buffer.extend_from_slice(&[0, 0, 0, 0]);
    read_buffer.extend_from_slice(body_and_tail);

    let fragments = prefixed_fragment_bytes.to_vec();
    let Some((buttons, final_cursor)) = parse_quickbar_read_buffer_with_fragments(
        &read_buffer,
        &fragments,
        LEGACY_QUICKBAR_READ_CURSOR_START,
    ) else {
        return None;
    };
    if final_cursor < LEGACY_QUICKBAR_READ_CURSOR_START || final_cursor > read_buffer.len() {
        return None;
    }

    let read_body_len = final_cursor.checked_sub(LEGACY_QUICKBAR_READ_CURSOR_START)?;
    let fragment_tail_len = body_and_tail.len().checked_sub(read_body_len)?;
    if fragment_tail_len <= MAX_DECOMPILE_OWNED_QUICKBAR_FRAGMENT_TAIL_SCAN_BYTES {
        return None;
    }

    let translated_item_slots = buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Item { .. }))
        .count() as u32;
    let spell_slots = buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
        .count() as u32;
    let general_slots = buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::General { .. }))
        .count() as u32;
    let item_candidate_slots = buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::ItemCandidate))
        .count() as u32;
    let unsupported_slots = buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Unsupported))
        .count() as u32;
    let split = QuickbarTransportSplit {
        read_body_len,
        fragment_tail_len,
        translated_item_slots,
        spell_slots,
        general_slots,
        item_candidate_slots,
        unsupported_slots,
        trailing_read_bytes: 0,
    };

    let reject_reason = match policy {
        QuickbarSplitPolicy::ExactSemantic => {
            if translated_item_slots == 0 && spell_slots == 0 {
                Some("exact-no-item-or-spell")
            } else if item_candidate_slots != 0 || unsupported_slots != 0 {
                Some("exact-blanked-or-unsupported")
            } else {
                None
            }
        }
        QuickbarSplitPolicy::DecompileOwnedBoundary => {
            if !split.preserves_decompile_owned_payload() {
                Some("boundary-no-owned-payload")
            } else if unsupported_slots != 0 {
                Some("boundary-unsupported-slots")
            } else {
                None
            }
        }
    };
    if let Some(reason) = reject_reason {
        tracing::info!(
            policy = ?policy,
            body_and_tail_len = body_and_tail.len(),
            read_body_len,
            fragment_tail_len,
            translated_item_slots,
            spell_slots,
            general_slots,
            item_candidate_slots,
            unsupported_slots,
            reason,
            "server GuiQuickbar_SetAllButtons cursor-derived split rejected"
        );
        return None;
    }

    tracing::info!(
        policy = ?policy,
        body_and_tail_len = body_and_tail.len(),
        read_body_len,
        fragment_tail_len,
        translated_item_slots,
        spell_slots,
        general_slots,
        item_candidate_slots,
        unsupported_slots,
        "server GuiQuickbar_SetAllButtons cursor-derived split accepted"
    );
    Some(split)
}

pub(super) fn choose_legacy_quickbar_item_end(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
    memo: &mut [i32],
) -> Option<usize> {
    let remaining_slots_after_this = LEGACY_QUICKBAR_BUTTON_COUNT.checked_sub(slot + 1)?;
    let item_payload_start = cursor.checked_add(1)?;
    let min_candidate = read_buffer.len().min(item_payload_start.checked_add(8)?);
    let max_candidate = read_buffer.len().min(item_payload_start.checked_add(512)?);
    let mut best_score = QUICKBAR_BAD_SCORE;
    let mut best_candidate = None;

    for candidate in min_candidate..=max_candidate {
        if candidate.checked_add(remaining_slots_after_this)? > read_buffer.len() {
            break;
        }
        if remaining_slots_after_this > 0
            && (candidate >= read_buffer.len()
                || !is_legacy_quickbar_plausible_type(read_buffer[candidate]))
        {
            continue;
        }

        let mut score = score_legacy_quickbar_parse_from(read_buffer, slot + 1, candidate, memo);
        if score <= QUICKBAR_BAD_SCORE / 2 {
            continue;
        }
        let skipped = candidate.saturating_sub(item_payload_start);
        score += 12 - skipped.checked_div(16).unwrap_or(0).min(120) as i32;
        if score > best_score {
            best_score = score;
            best_candidate = Some(candidate);
        }
    }

    if best_score < 0 { None } else { best_candidate }
}

fn score_legacy_quickbar_parse_from(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
    memo: &mut [i32],
) -> i32 {
    if cursor > read_buffer.len() || slot > LEGACY_QUICKBAR_BUTTON_COUNT {
        return QUICKBAR_BAD_SCORE;
    }
    if slot == LEGACY_QUICKBAR_BUTTON_COUNT {
        let unread = cursor.abs_diff(read_buffer.len());
        return 100_000 - unread.min(10_000) as i32 * 25;
    }
    if cursor >= read_buffer.len() {
        return QUICKBAR_BAD_SCORE;
    }

    let memo_width = read_buffer.len() + 1;
    let memo_index = slot
        .checked_mul(memo_width)
        .and_then(|base| base.checked_add(cursor));
    if let Some(index) = memo_index {
        if let Some(score) = memo.get(index).copied() {
            if score != QUICKBAR_UNKNOWN_SCORE {
                return score;
            }
        }
    }

    let ty = read_buffer[cursor];
    let remaining_slots_after_this = LEGACY_QUICKBAR_BUTTON_COUNT - slot - 1;
    let mut best_score = QUICKBAR_BAD_SCORE;

    if ty == 1 {
        let item_payload_start = cursor + 1;
        let min_candidate = read_buffer.len().min(item_payload_start.saturating_add(8));
        let max_candidate = read_buffer
            .len()
            .min(item_payload_start.saturating_add(420));
        for candidate in min_candidate..=max_candidate {
            if candidate.saturating_add(remaining_slots_after_this) > read_buffer.len() {
                break;
            }
            if remaining_slots_after_this > 0
                && (candidate >= read_buffer.len()
                    || !is_legacy_quickbar_plausible_type(read_buffer[candidate]))
            {
                continue;
            }

            let mut score =
                score_legacy_quickbar_parse_from(read_buffer, slot + 1, candidate, memo);
            if score <= QUICKBAR_BAD_SCORE / 2 {
                continue;
            }
            let skipped = candidate.saturating_sub(item_payload_start);
            score += 12 - skipped.checked_div(16).unwrap_or(0).min(120) as i32;
            best_score = best_score.max(score);
        }
    } else if let Some((button, next_cursor)) = parse_legacy_quickbar_non_item(read_buffer, cursor)
    {
        let mut score = score_legacy_quickbar_parse_from(read_buffer, slot + 1, next_cursor, memo);
        if score > QUICKBAR_BAD_SCORE / 2 {
            match button.kind {
                QuickbarButtonKind::Spell { .. } => score += 60,
                QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0 => {
                    score += 8;
                }
                QuickbarButtonKind::General { ref bytes }
                    if bytes.len() == 1 && legacy_quickbar_type_has_no_payload(bytes[0]) =>
                {
                    score += 20;
                }
                QuickbarButtonKind::General { .. } | QuickbarButtonKind::Unsupported => {
                    score += 4;
                }
                QuickbarButtonKind::Item { .. } => {
                    score += 100;
                }
                QuickbarButtonKind::ItemCandidate => {}
            }
            best_score = score;
        }
    }

    if let Some(index) = memo_index {
        if let Some(slot) = memo.get_mut(index) {
            *slot = best_score;
        }
    }
    best_score
}

fn score_legacy_quickbar_candidate_window(
    read_buffer: &[u8],
    mut cursor: usize,
    remaining_slots: usize,
) -> Option<i32> {
    if cursor >= read_buffer.len() || remaining_slots == 0 {
        return None;
    }

    let mut score = 0;
    let slots_to_probe = remaining_slots.min(8);
    for probe in 0..slots_to_probe {
        if cursor >= read_buffer.len() {
            return if probe == 0 { None } else { Some(score - 20) };
        }

        let ty = read_buffer[cursor];
        if !is_legacy_quickbar_plausible_type(ty) {
            return None;
        }
        if ty == 1 {
            return Some(score + 12);
        }

        let (button, next_cursor) = parse_legacy_quickbar_non_item(read_buffer, cursor)?;
        if next_cursor <= cursor {
            return None;
        }
        match button.kind {
            QuickbarButtonKind::Spell { .. } => score += 80,
            QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0 => {
                score += 8
            }
            QuickbarButtonKind::General { .. } => score += 4,
            QuickbarButtonKind::Item { .. } => score += 100,
            QuickbarButtonKind::ItemCandidate | QuickbarButtonKind::Unsupported => return None,
        }
        cursor = next_cursor;
    }

    Some(score)
}

pub(super) fn find_legacy_quickbar_resync(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
) -> Option<usize> {
    let remaining_slots = LEGACY_QUICKBAR_BUTTON_COUNT.checked_sub(slot)?;
    let max_candidate = read_buffer.len().min(cursor.checked_add(2048)?);
    let mut best_score = i32::MIN;
    let mut best_candidate = None;

    for candidate in cursor.saturating_add(1)..max_candidate {
        let ty = read_buffer[candidate];
        if !is_legacy_quickbar_plausible_type(ty) || ty == 1 {
            continue;
        }

        let mut score =
            score_legacy_quickbar_candidate_window(read_buffer, candidate, remaining_slots)?;
        if ty == 2 {
            score += 120;
        } else if ty == 0 {
            score += 4;
        } else if legacy_quickbar_type_has_no_payload(ty) {
            score += 12;
        }
        let skipped = candidate.saturating_sub(cursor);
        score -= skipped.checked_div(64).unwrap_or(0).min(40) as i32;

        if score > best_score {
            best_score = score;
            best_candidate = Some(candidate);
        }
    }

    if best_score >= 80 {
        best_candidate
    } else {
        None
    }
}
