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
    fn classified_slots(&self) -> u32 {
        self.translated_item_slots
            .saturating_add(self.spell_slots)
            .saturating_add(self.general_slots)
            .saturating_add(self.item_candidate_slots)
            .saturating_add(self.unsupported_slots)
    }

    fn has_decompile_owned_item_or_spell(&self) -> bool {
        self.translated_item_slots != 0 || self.spell_slots != 0
    }

    fn has_discardable_decompile_owned_trailing_read_bytes(&self) -> bool {
        self.trailing_read_bytes <= MAX_DECOMPILE_OWNED_QUICKBAR_FRAGMENT_TAIL_SCAN_BYTES
            && self.has_decompile_owned_item_or_spell()
            && self.unsupported_slots == 0
            && self.classified_slots() == LEGACY_QUICKBAR_BUTTON_COUNT as u32
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
    /// writer emits empty EE slots for unowned `ItemCandidate`/`Unsupported`
    /// source spans. This is still strict translation: no raw candidate bytes
    /// are forwarded, and blanking can only claim the packet after at least one
    /// decompile-owned item/spell slot proves the cursor phase. Most captures
    /// must consume the read buffer exactly. A narrow local-Diamond shape may
    /// carry a zero CNW declared offset and a small source-only read suffix;
    /// that suffix is discarded only after the reader has classified all 36
    /// slots and the EE writer/validator own the emitted quickbar shape.
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
        let mut read_buffer = Vec::with_capacity(read_body_len);
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
                } else if trailing_read_bytes != 0
                    && !split.has_discardable_decompile_owned_trailing_read_bytes()
                {
                    Some("boundary-trailing-read-bytes")
                } else {
                    if trailing_read_bytes != 0 {
                        tracing::info!(
                            read_body_len,
                            fragment_tail_len,
                            translated_item_slots,
                            spell_slots,
                            general_slots,
                            item_candidate_slots,
                            trailing_read_bytes,
                            "server GuiQuickbar_SetAllButtons accepted decompile-owned zero-declared split with discardable source read suffix"
                        );
                    }
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

        if matches!(policy, QuickbarSplitPolicy::DecompileOwnedBoundary)
            && prefixed_fragment_bytes.len() == CNW_LENGTH_BYTES
            && split.fragment_tail_len == 0
            && split.unsupported_slots == 0
            && split.classified_slots() == LEGACY_QUICKBAR_BUTTON_COUNT as u32
            && split.has_decompile_owned_item_or_spell()
            && (split.trailing_read_bytes == 0
                || split.has_discardable_decompile_owned_trailing_read_bytes())
        {
            // Local Diamond zero-declared SetAllButtons puts the four source
            // bytes after `P 1E 01` in the prefixed fragment position, then
            // follows with the 36-slot read body. Once the decompile-owned
            // reader has classified all slots from that exact tail-0 phase,
            // continuing to slide bytes from the read body into the fragment
            // tail only fabricates alternative candidate scores. Return the
            // typed boundary immediately: no raw bytes are forwarded, and the
            // EE writer/validator still own the emitted quickbar shape.
            tracing::info!(
                read_body_len = split.read_body_len,
                fragment_tail_len = split.fragment_tail_len,
                translated_item_slots = split.translated_item_slots,
                spell_slots = split.spell_slots,
                general_slots = split.general_slots,
                item_candidate_slots = split.item_candidate_slots,
                trailing_read_bytes = split.trailing_read_bytes,
                "server GuiQuickbar_SetAllButtons accepted zero-declared decompile-owned split without tail sweep"
            );
            return Some(split);
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
    let mut read_buffer = Vec::with_capacity(body_and_tail.len());
    read_buffer.extend_from_slice(body_and_tail);

    let fragments = prefixed_fragment_bytes.to_vec();
    let Some((buttons, final_cursor)) = parse_quickbar_read_buffer_with_fragments(
        &read_buffer,
        &fragments,
        LEGACY_QUICKBAR_READ_CURSOR_START,
    ) else {
        return None;
    };
    if final_cursor > read_buffer.len() {
        return None;
    }

    let read_body_len = final_cursor;
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
            if !split.has_decompile_owned_item_or_spell() {
                // Keep this fallback aligned with the normal bounded tail
                // sweep above: general/no-payload buttons and item-candidate
                // blanks are not enough proof because a shifted byte cursor can
                // manufacture them cheaply.  The split must preserve at least
                // one decompile-owned item or spell slot before any source-only
                // fragment tail is discarded.
                Some("boundary-no-item-or-spell")
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
    model_types: &[i8],
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
        if remaining_slots_after_this > 0 && candidate >= read_buffer.len() {
            continue;
        }

        let mut score =
            score_legacy_quickbar_parse_from(read_buffer, slot + 1, candidate, model_types, memo);
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

pub(super) fn choose_legacy_quickbar_compact_item_end(
    read_buffer: &[u8],
    slot: usize,
    payload_start: usize,
    model_types: &[i8],
    memo: &mut [i32],
) -> Option<(usize, QuickbarItemObject, QuickbarItemObject)> {
    let remaining_slots_after_this = LEGACY_QUICKBAR_BUTTON_COUNT.checked_sub(slot + 1)?;
    let min_candidate = read_buffer.len().min(payload_start.checked_add(8)?);
    let max_candidate = read_buffer.len().min(payload_start.checked_add(512)?);
    let mut best_score = QUICKBAR_BAD_SCORE;
    let mut best = None;

    for candidate in min_candidate..=max_candidate {
        if candidate.checked_add(remaining_slots_after_this)? > read_buffer.len() {
            break;
        }
        if remaining_slots_after_this > 0 && candidate >= read_buffer.len() {
            continue;
        }
        let Some((primary, secondary)) = parse_legacy_quickbar_compact_byte_item_payload(
            read_buffer,
            payload_start,
            candidate,
            model_types,
        ) else {
            continue;
        };

        let mut score =
            score_legacy_quickbar_parse_from(read_buffer, slot + 1, candidate, model_types, memo);
        if score <= QUICKBAR_BAD_SCORE / 2 {
            continue;
        }
        let consumed = candidate.saturating_sub(payload_start);
        score += 120 - consumed.checked_div(16).unwrap_or(0).min(120) as i32;
        if score > best_score {
            best_score = score;
            best = Some((candidate, primary, secondary));
        }
    }

    if best_score < 0 { None } else { best }
}

fn score_legacy_quickbar_parse_from(
    read_buffer: &[u8],
    slot: usize,
    cursor: usize,
    model_types: &[i8],
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
            if remaining_slots_after_this > 0 && candidate >= read_buffer.len() {
                continue;
            }

            let mut score = score_legacy_quickbar_parse_from(
                read_buffer,
                slot + 1,
                candidate,
                model_types,
                memo,
            );
            if score <= QUICKBAR_BAD_SCORE / 2 {
                continue;
            }
            let skipped = candidate.saturating_sub(item_payload_start);
            score += 12 - skipped.checked_div(16).unwrap_or(0).min(120) as i32;
            best_score = best_score.max(score);
        }
    } else {
        // HG/Diamond captures can contain compact item bodies at a 36-slot
        // button boundary without the explicit source type-1 byte.  This is
        // not treated as passthrough: a candidate only scores if the typed
        // compact item-object parser owns the full candidate window and the
        // remaining slots also parse to a complete quickbar stream.  The
        // reader will later re-emit this model through the EE writer, which
        // restores the decompile-required type byte.
        let min_candidate = read_buffer.len().min(cursor.saturating_add(8));
        let max_candidate = read_buffer.len().min(cursor.saturating_add(512));
        for candidate in min_candidate..=max_candidate {
            if candidate.saturating_add(remaining_slots_after_this) > read_buffer.len() {
                break;
            }
            if remaining_slots_after_this > 0 && candidate >= read_buffer.len() {
                continue;
            }

            let Some((_primary, _secondary)) = parse_legacy_quickbar_compact_byte_item_payload(
                read_buffer,
                cursor,
                candidate,
                model_types,
            ) else {
                continue;
            };
            let mut score = score_legacy_quickbar_parse_from(
                read_buffer,
                slot + 1,
                candidate,
                model_types,
                memo,
            );
            if score <= QUICKBAR_BAD_SCORE / 2 {
                continue;
            }
            let consumed = candidate.saturating_sub(cursor);
            score += 100 - consumed.checked_div(16).unwrap_or(0).min(100) as i32;
            best_score = best_score.max(score);
        }
    }

    if ty != 1
        && let Some((button, next_cursor)) = parse_legacy_quickbar_non_item(read_buffer, cursor)
    {
        let mut score =
            score_legacy_quickbar_parse_from(read_buffer, slot + 1, next_cursor, model_types, memo);
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
            best_score = best_score.max(score);
        }
    }

    if let Some(index) = memo_index {
        if let Some(slot) = memo.get_mut(index) {
            *slot = best_score;
        }
    }
    best_score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_derived_split_rejects_candidate_only_quickbar_tail() {
        let mut body_and_tail = Vec::new();
        body_and_tail.push(1);
        body_and_tail.extend(std::iter::repeat(0).take(8));
        body_and_tail.extend(std::iter::repeat(0).take(LEGACY_QUICKBAR_BUTTON_COUNT - 1));
        body_and_tail.extend(
            std::iter::repeat(0xFF)
                .take(MAX_DECOMPILE_OWNED_QUICKBAR_FRAGMENT_TAIL_SCAN_BYTES + 16),
        );

        let split = choose_quickbar_split(
            &body_and_tail,
            &[0x60],
            QuickbarSplitPolicy::DecompileOwnedBoundary,
        );

        assert!(
            split.is_none(),
            "a cursor-derived split with only blank/candidate slots must not discard a source tail"
        );
    }

    #[test]
    fn split_rejects_resynced_leading_byte_before_spell_slot() {
        let mut body_and_tail = Vec::new();
        body_and_tail.push(0xFF);
        body_and_tail.push(2);
        body_and_tail.push(0);
        body_and_tail.extend_from_slice(&1u32.to_le_bytes());
        body_and_tail.push(0);
        body_and_tail.push(0);
        body_and_tail.extend(std::iter::repeat(0).take(LEGACY_QUICKBAR_BUTTON_COUNT - 1));

        let split = choose_quickbar_split(
            &body_and_tail,
            &[0x60],
            QuickbarSplitPolicy::DecompileOwnedBoundary,
        );

        assert!(
            split.is_none(),
            "SetAllButtons has no Diamond/EE reader branch that skips an unowned byte before slot 0"
        );
    }
}
