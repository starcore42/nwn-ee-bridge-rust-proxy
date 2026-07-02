use super::*;

// Public quickbar translation facade. These functions are the only entry points
// used by the M-frame dispatcher; all semantic work is delegated to the focused
// reader/writer modules.

pub fn full_set_all_buttons_target_length(payload: &[u8]) -> Option<usize> {
    let high = HighLevel::parse(payload)?;
    if is_quickbar_family(high) {
        // `CNWMessage::SetReadMessage` uses the declared value as the fragment
        // offset from the start of the `P major minor` payload. That proves
        // where the fragment tail starts only after the whole M/zlib window has
        // delivered at least that many bytes. Returning a target length here
        // would risk flushing a split packet before the BOOL fragments arrive.
        return None;
    }
    None
}

pub fn rewrite_simple_quickbar_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<QuickbarRewriteSummary> {
    rewrite_simple_quickbar_payload_with_context_if_possible(payload, None)
}

pub(crate) fn rewrite_simple_quickbar_payload_with_context_for_stream_probe_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<QuickbarRewriteSummary> {
    rewrite_simple_quickbar_payload_with_context_and_trace_if_possible(
        payload,
        materialization,
        QuickbarRewriteTraceRole::StreamProbe,
    )
}

pub fn rewrite_simple_quickbar_payload_with_context_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<QuickbarRewriteSummary> {
    rewrite_simple_quickbar_payload_with_context_and_trace_if_possible(
        payload,
        materialization,
        QuickbarRewriteTraceRole::Committed,
    )
}

fn rewrite_simple_quickbar_payload_with_context_and_trace_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
    trace_role: QuickbarRewriteTraceRole,
) -> Option<QuickbarRewriteSummary> {
    let old_payload_length = payload.len();
    let parsed = parse_cnw_quickbar_payload(payload)?;
    dump_quickbar_payload("simple_before", payload);
    let old_declared = parsed.declared;
    let rewritten =
        match super::writer::build_ee_quickbar_payload_with_context(&parsed, materialization) {
            Some(rewritten) => rewritten,
            None => return None,
        };
    let summary = summarize_quickbar_rewrite(
        &parsed,
        old_payload_length,
        rewritten.len(),
        old_declared,
        read_le_u32(&rewritten, HIGH_LEVEL_HEADER_BYTES).unwrap_or(old_declared),
        materialization,
    );
    trace_quickbar_item_button_decisions("simple", &parsed, materialization, trace_role);
    trace_quickbar_rewrite_summary("simple", &summary, trace_role);
    trace_quickbar_materialization_context("simple", materialization, &summary, trace_role);
    dump_quickbar_payload("simple_after", &rewritten);
    *payload = rewritten;
    Some(summary)
}

pub(super) fn quickbar_has_plausible_cnw_declared(payload: &[u8]) -> bool {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES {
        return false;
    }
    let Some(declared) = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES) else {
        return false;
    };
    let Ok(declared) = usize::try_from(declared) else {
        return false;
    };
    let read_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    if declared < read_start || declared > MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES {
        return false;
    }
    if declared >= payload.len() {
        return false;
    }
    let read_buffer = &payload[read_start..declared];
    let fragments = &payload[declared..];
    quickbar_read_window_parses(read_buffer, fragments)
}

pub fn normalize_and_rewrite_quickbar_payload_if_possible(
    payload: &mut Vec<u8>,
) -> Option<(PrefixedFragmentsNormalizeSummary, QuickbarRewriteSummary)> {
    normalize_and_rewrite_quickbar_payload_with_context_if_possible(payload, None)
}

pub(crate) fn normalize_and_rewrite_quickbar_payload_with_context_for_stream_probe_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<(PrefixedFragmentsNormalizeSummary, QuickbarRewriteSummary)> {
    normalize_and_rewrite_quickbar_payload_with_context_and_trace_if_possible(
        payload,
        materialization,
        QuickbarRewriteTraceRole::StreamProbe,
    )
}

pub fn normalize_and_rewrite_quickbar_payload_with_context_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<(PrefixedFragmentsNormalizeSummary, QuickbarRewriteSummary)> {
    normalize_and_rewrite_quickbar_payload_with_context_and_trace_if_possible(
        payload,
        materialization,
        QuickbarRewriteTraceRole::Committed,
    )
}

fn normalize_and_rewrite_quickbar_payload_with_context_and_trace_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
    trace_role: QuickbarRewriteTraceRole,
) -> Option<(PrefixedFragmentsNormalizeSummary, QuickbarRewriteSummary)> {
    let mut normalized = payload.clone();
    let normalize = normalize_quickbar_payload_if_needed(&mut normalized)?;
    let old_payload_length = normalized.len();
    let parsed = parse_cnw_quickbar_payload(&normalized)?;
    dump_quickbar_payload("normalized_before", &normalized);
    let old_declared = parsed.declared;
    let rewritten =
        super::writer::build_ee_quickbar_payload_with_context(&parsed, materialization)?;
    let new_declared = read_le_u32(&rewritten, HIGH_LEVEL_HEADER_BYTES).unwrap_or(old_declared);
    let summary = summarize_quickbar_rewrite(
        &parsed,
        old_payload_length,
        rewritten.len(),
        old_declared,
        new_declared,
        materialization,
    );
    trace_quickbar_item_button_decisions("normalized", &parsed, materialization, trace_role);
    trace_quickbar_rewrite_summary("normalized", &summary, trace_role);
    trace_quickbar_materialization_context("normalized", materialization, &summary, trace_role);
    dump_quickbar_payload("normalized_after", &rewritten);
    *payload = rewritten;
    Some((normalize, summary))
}

pub fn rewrite_summary_needs_more_quickbar_bytes(summary: &QuickbarRewriteSummary) -> bool {
    // This only gates stream buffering. A fully owned quickbar parse consumes its
    // read buffer and either preserves spells/items or deliberately blanks
    // unsupported slots. If the cursor did not reach the declared read size, the
    // packet likely arrived split across deflated windows and should wait.
    //
    // Do not treat deliberately blanked item/unsupported slots as evidence that
    // more zlib stream bytes are required. The quickbar reader has already
    // proven a 36-slot decompile-owned boundary before the writer can emit
    // blank EE slots for unowned source spans. Waiting solely because many
    // source slots were blanked turns a valid partial semantic salvage into the
    // visible `GuiQuickbarPlaceholder` regression that clears the whole bar.
    if summary.trailing_read_bytes == 0 {
        return false;
    }

    if quickbar_summary_has_complete_decompile_owned_slot_shape(summary) {
        tracing::info!(
            read_size = summary.read_size,
            final_cursor = summary.final_cursor,
            trailing_read_bytes = summary.trailing_read_bytes,
            slot_records_owned = summary.slot_records_owned,
            fragment_size = summary.fragment_size,
            spells_preserved = summary.spells_preserved,
            item_buttons_blanked = summary.item_buttons_blanked,
            unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
            "server GuiQuickbar_SetAllButtons trailing legacy read bytes are owned by the verified 36-slot translator; not waiting for zlib continuation"
        );
        return false;
    }

    summary.new_payload_length < MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES
}

fn quickbar_summary_has_complete_decompile_owned_slot_shape(
    summary: &QuickbarRewriteSummary,
) -> bool {
    // Diamond and EE both read SetAllButtons as a fixed 36-slot quickbar body followed by
    // the CNW fragment tail. Once the bounded reader has produced that typed slot model and
    // the writer has emitted an exact EE quickbar payload, bytes left inside the legacy read
    // buffer are not treated as a zlib continuation. They are legacy slot/item subobject tails
    // that the semantic translator either preserved in typed form or deliberately blanked by
    // policy. Truly split/incomplete quickbar streams still arrive without this completed
    // fragment-tail proof and continue down the buffering path.
    !summary.direct_opcode_stream
        && summary.fragment_size != 0
        && summary.final_cursor <= summary.read_size
        && (summary.blank_buttons_seen != 0
            || summary.spells_preserved != 0
            || summary.item_buttons_preserved != 0
            || summary.general_buttons_preserved != 0
            || summary.general_buttons_blanked != 0
            || summary.item_buttons_blanked != 0
            || summary.unsupported_buttons_blanked != 0)
}

pub(in crate::translate::quickbar) fn parse_cnw_quickbar_payload(
    payload: &[u8],
) -> Option<QuickbarParse> {
    if payload.len() < HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES {
        return parse_direct_opcode_quickbar_stream(payload);
    }
    let high = HighLevel::parse(payload)?;
    if !is_quickbar_family(high) {
        return None;
    }
    let Some(declared) = read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES) else {
        return parse_direct_opcode_quickbar_stream(payload);
    };
    let Some(declared_usize) = usize::try_from(declared).ok() else {
        return parse_direct_opcode_quickbar_stream(payload);
    };
    if declared_usize < HIGH_LEVEL_HEADER_BYTES {
        return parse_direct_opcode_quickbar_stream(payload);
    }
    parse_cnw_quickbar_payload_with_ee_declared(payload, high, declared, declared_usize).or_else(
        || {
            parse_cnw_quickbar_payload_with_legacy_short_declared(
                payload,
                high,
                declared,
                declared_usize,
            )
        },
    )
}

fn parse_cnw_quickbar_payload_with_ee_declared(
    payload: &[u8],
    high: HighLevel,
    declared: u32,
    declared_usize: usize,
) -> Option<QuickbarParse> {
    let read_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    if declared_usize < read_start || declared_usize > payload.len() {
        return None;
    }
    let read_buffer = payload.get(read_start..declared_usize)?;
    if read_buffer.len() > MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES {
        return None;
    }
    let fragments = payload.get(declared_usize..)?;
    parse_quickbar_split(high, declared, read_buffer, fragments)
}

fn parse_cnw_quickbar_payload_with_legacy_short_declared(
    payload: &[u8],
    high: HighLevel,
    declared: u32,
    declared_usize: usize,
) -> Option<QuickbarParse> {
    // HG/1.69 captures observed before EE emission use a Diamond-compatible
    // short declared value for quickbar: `3 + read_bytes`, with the four-byte
    // CNW length field not counted in the fragment offset. This parser owns
    // that legacy source shape only so the writer can emit the EE
    // `SetReadMessage` shape (`3 + 4 + read_bytes`). The exact EE validator
    // below does not accept this source-only short form.
    let read_size = declared_usize.checked_sub(HIGH_LEVEL_HEADER_BYTES)?;
    if read_size > MAX_REASONABLE_REASSEMBLED_QUICKBAR_BYTES {
        return None;
    }
    let read_start = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
    let read_end = read_start.checked_add(read_size)?;
    if read_end > payload.len() {
        return None;
    }
    let read_buffer = payload.get(read_start..read_end)?;
    let fragments = payload.get(read_end..)?;
    parse_quickbar_split(high, declared, read_buffer, fragments)
}

fn parse_quickbar_split(
    high: HighLevel,
    declared: u32,
    read_buffer: &[u8],
    fragments: &[u8],
) -> Option<QuickbarParse> {
    let fragment_size = fragments.len();
    let cursor = if read_buffer.len() >= LEGACY_QUICKBAR_READ_CURSOR_START {
        LEGACY_QUICKBAR_READ_CURSOR_START
    } else {
        0
    };
    let parsed = if fragment_size != 0 {
        parse_quickbar_read_buffer_with_fragments(read_buffer, fragments, cursor)
    } else {
        parse_quickbar_read_buffer(read_buffer, cursor)
    };
    let (buttons, final_cursor) = parsed.or_else(|| {
        parse_quickbar_split_with_one_unused_fragment_storage_byte(read_buffer, fragments, cursor)
    })?;
    Some(QuickbarParse {
        envelope: high.envelope,
        declared,
        read_size: read_buffer.len(),
        fragment_size,
        final_cursor,
        buttons,
        direct_opcode_stream: false,
    })
}

fn parse_quickbar_split_with_one_unused_fragment_storage_byte(
    read_buffer: &[u8],
    fragments: &[u8],
    cursor: usize,
) -> Option<(Vec<QuickbarButton>, usize)> {
    if fragments.len() != 2 {
        return None;
    }
    let trimmed = fragments.get(..1)?;
    let (buttons, final_cursor) =
        parse_quickbar_read_buffer_with_fragments(read_buffer, trimmed, cursor)?;
    if final_cursor != read_buffer.len() {
        return None;
    }
    let has_owned_item_or_spell = buttons.iter().any(|button| {
        matches!(
            button.kind,
            QuickbarButtonKind::Item { .. } | QuickbarButtonKind::Spell { .. }
        )
    });
    let has_unsupported = buttons
        .iter()
        .any(|button| matches!(button.kind, QuickbarButtonKind::Unsupported));
    if !has_owned_item_or_spell || has_unsupported {
        return None;
    }

    // Local Diamond XP2 Chapter 3 proves this narrow storage form: the source
    // carries two bytes after the declared quickbar read window, but the first
    // byte's final-bit cursor and the decompiled 36-slot reader already consume
    // the complete fragment stream. The second byte is accepted only as
    // boundary proof; the writer emits a fresh EE fragment tail from the typed
    // model and never forwards either source byte raw.
    tracing::info!(
        source_fragment_bytes = fragments.len(),
        consumed_fragment_bytes = trimmed.len(),
        read_buffer_len = read_buffer.len(),
        "server GuiQuickbar_SetAllButtons accepted one unused source fragment storage byte after exact slot proof"
    );
    Some((buttons, final_cursor))
}

fn quickbar_read_window_parses(read_buffer: &[u8], fragments: &[u8]) -> bool {
    let cursor = if read_buffer.len() >= LEGACY_QUICKBAR_READ_CURSOR_START {
        LEGACY_QUICKBAR_READ_CURSOR_START
    } else {
        0
    };
    if fragments.is_empty() {
        parse_quickbar_read_buffer(read_buffer, cursor).is_some()
    } else {
        parse_quickbar_read_buffer_with_fragments(read_buffer, fragments, cursor).is_some()
    }
}

fn summarize_quickbar_rewrite(
    parsed: &QuickbarParse,
    old_payload_length: usize,
    new_payload_length: usize,
    old_declared: u32,
    new_declared: u32,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> QuickbarRewriteSummary {
    let mut item_buttons_total = 0_u32;
    let mut item_buttons_source_explicit = 0_u32;
    let mut item_buttons_source_compact = 0_u32;
    let mut item_buttons_source_recovered = 0_u32;
    let mut item_buttons_emitted = 0_u32;
    let mut rejection_counts = QuickbarItemRejectionCounts::default();
    let mut missing_state_object_counts = QuickbarMissingStateObjectCounts::default();
    let mut materialization_counts = QuickbarMaterializationCounts::default();
    for button in &parsed.buttons {
        let QuickbarButtonKind::Item {
            primary,
            secondary,
            source,
            recovered_type_tag,
        } = &button.kind
        else {
            continue;
        };
        item_buttons_total = item_buttons_total.saturating_add(1);
        match source {
            QuickbarItemSource::ExplicitTypeAndFragmentBits => {
                item_buttons_source_explicit = item_buttons_source_explicit.saturating_add(1);
            }
            QuickbarItemSource::CompactByteOwnedWithSourceType => {
                item_buttons_source_compact = item_buttons_source_compact.saturating_add(1);
            }
            QuickbarItemSource::RecoveredMissingType => {
                item_buttons_source_recovered = item_buttons_source_recovered.saturating_add(1);
            }
        }
        let proofs = match super::writer::quickbar_item_button_materialization_decision(
            primary,
            secondary,
            *source,
            *recovered_type_tag,
            materialization,
        ) {
            Ok(proofs) => proofs,
            Err(reason) => {
                if reason
                    == super::writer::QuickbarItemMaterializationRejectReason::MissingStateProof
                {
                    rejection_counts.observe_missing_state_status(
                        super::writer::quickbar_item_button_missing_state_status(
                            primary,
                            secondary,
                            materialization,
                        ),
                    );
                    missing_state_object_counts.observe(
                        super::writer::quickbar_item_button_missing_state_object_statuses(
                            primary,
                            secondary,
                            materialization,
                        ),
                    );
                }
                rejection_counts.observe(reason);
                continue;
            }
        };
        item_buttons_emitted = item_buttons_emitted.saturating_add(1);
        materialization_counts.observe(proofs);
    }
    let item_buttons_blanked_by_policy = item_buttons_total.saturating_sub(item_buttons_emitted);
    let spells_preserved = parsed
        .buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
        .count() as u32;
    let blank_buttons_seen = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0
            )
        })
        .count() as u32;
    let general_buttons_preserved = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::General { ref bytes }
                    if quickbar_general_bytes_are_verified_ee_identical(bytes)
                        && !(bytes.len() == 1 && bytes[0] == 0)
            )
        })
        .count() as u32;
    let general_buttons_blanked = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::General { ref bytes }
                    if !quickbar_general_bytes_are_verified_ee_identical(bytes)
            )
        })
        .count() as u32;
    let item_candidate_buttons = parsed
        .buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::ItemCandidate))
        .count() as u32;
    let unsupported_buttons_blanked = parsed
        .buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Unsupported))
        .count() as u32;
    let slot_records_owned = parsed
        .buttons
        .iter()
        .filter(|button| !matches!(button.kind, QuickbarButtonKind::Unsupported))
        .count() as u32;
    QuickbarRewriteSummary {
        old_payload_length,
        new_payload_length,
        old_declared,
        new_declared,
        read_size: parsed.read_size,
        fragment_size: parsed.fragment_size,
        final_cursor: parsed.final_cursor,
        trailing_read_bytes: parsed.read_size.saturating_sub(parsed.final_cursor),
        direct_opcode_stream: parsed.direct_opcode_stream,
        slot_records_owned,
        item_buttons_seen: item_buttons_total,
        item_buttons_source_explicit,
        item_buttons_source_compact,
        item_buttons_source_recovered,
        item_buttons_preserved: item_buttons_emitted,
        spells_preserved,
        blank_buttons_seen,
        general_buttons_preserved,
        general_buttons_blanked,
        item_buttons_blanked: item_candidate_buttons.saturating_add(item_buttons_blanked_by_policy),
        item_buttons_blanked_candidate: item_candidate_buttons,
        unsupported_buttons_blanked,
        item_buttons_rejected_recovered_type_tag: rejection_counts.recovered_type_tag,
        item_buttons_rejected_missing_type_source: rejection_counts.missing_type_source,
        item_buttons_rejected_no_present_item: rejection_counts.no_present_item,
        item_buttons_rejected_invalid_object_id: rejection_counts.invalid_object_id,
        item_buttons_rejected_missing_active_properties: rejection_counts.missing_active_properties,
        item_buttons_rejected_unsupported_appearance_type: rejection_counts
            .unsupported_appearance_type,
        item_buttons_rejected_appearance_shape: rejection_counts.appearance_shape,
        item_buttons_rejected_missing_state_proof: rejection_counts.missing_state_proof,
        item_buttons_rejected_missing_state_unknown: rejection_counts.missing_state_unknown,
        item_buttons_rejected_missing_state_cleared_delete: rejection_counts
            .missing_state_cleared_delete,
        item_buttons_rejected_missing_state_cleared_area_reset: rejection_counts
            .missing_state_cleared_area_reset,
        item_objects_rejected_missing_state_proven: missing_state_object_counts.proven,
        item_objects_rejected_missing_state_active: missing_state_object_counts.active_state,
        item_objects_rejected_missing_state_feature25_first: missing_state_object_counts
            .feature25_first,
        item_objects_rejected_missing_state_feature25_second: missing_state_object_counts
            .feature25_second,
        item_objects_rejected_missing_state_feature25_legacy_tail: missing_state_object_counts
            .feature25_legacy_tail,
        item_objects_rejected_missing_state_unknown: missing_state_object_counts.unknown,
        item_objects_rejected_missing_state_cleared_delete: missing_state_object_counts
            .cleared_delete,
        item_objects_rejected_missing_state_cleared_area_reset: missing_state_object_counts
            .cleared_area_reset,
        item_objects_preserved_by_explicit_self_materialization: materialization_counts
            .explicit_self_materialization,
        item_objects_preserved_by_active_state: materialization_counts.active_state,
        item_objects_preserved_by_feature25_first: materialization_counts.feature25_first,
        item_objects_preserved_by_feature25_second: materialization_counts.feature25_second,
        item_objects_preserved_by_feature25_legacy_tail: materialization_counts
            .feature25_legacy_tail,
    }
}

#[derive(Default)]
struct QuickbarItemRejectionCounts {
    recovered_type_tag: u32,
    missing_type_source: u32,
    no_present_item: u32,
    invalid_object_id: u32,
    missing_active_properties: u32,
    unsupported_appearance_type: u32,
    appearance_shape: u32,
    missing_state_proof: u32,
    missing_state_unknown: u32,
    missing_state_cleared_delete: u32,
    missing_state_cleared_area_reset: u32,
}

impl QuickbarItemRejectionCounts {
    fn observe(&mut self, reason: super::writer::QuickbarItemMaterializationRejectReason) {
        match reason {
            super::writer::QuickbarItemMaterializationRejectReason::RecoveredTypeTag => {
                self.recovered_type_tag = self.recovered_type_tag.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::MissingTypeSource => {
                self.missing_type_source = self.missing_type_source.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::NoPresentItem => {
                self.no_present_item = self.no_present_item.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::InvalidObjectId => {
                self.invalid_object_id = self.invalid_object_id.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::MissingActiveProperties => {
                self.missing_active_properties = self.missing_active_properties.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::UnsupportedAppearanceType => {
                self.unsupported_appearance_type =
                    self.unsupported_appearance_type.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::AppearanceShape => {
                self.appearance_shape = self.appearance_shape.saturating_add(1);
            }
            super::writer::QuickbarItemMaterializationRejectReason::MissingStateProof => {
                self.missing_state_proof = self.missing_state_proof.saturating_add(1);
            }
        }
    }

    fn observe_missing_state_status(&mut self, status: QuickbarItemMaterializationStatus) {
        match status {
            QuickbarItemMaterializationStatus::Proven(_) => {}
            QuickbarItemMaterializationStatus::ClearedByItemDelete => {
                self.missing_state_cleared_delete =
                    self.missing_state_cleared_delete.saturating_add(1);
            }
            QuickbarItemMaterializationStatus::ClearedByAreaReset => {
                self.missing_state_cleared_area_reset =
                    self.missing_state_cleared_area_reset.saturating_add(1);
            }
            QuickbarItemMaterializationStatus::Unknown => {
                self.missing_state_unknown = self.missing_state_unknown.saturating_add(1);
            }
        }
    }
}

#[derive(Default)]
struct QuickbarMissingStateObjectCounts {
    proven: u32,
    active_state: u32,
    feature25_first: u32,
    feature25_second: u32,
    feature25_legacy_tail: u32,
    unknown: u32,
    cleared_delete: u32,
    cleared_area_reset: u32,
}

impl QuickbarMissingStateObjectCounts {
    fn observe(&mut self, statuses: [Option<QuickbarItemMaterializationStatus>; 2]) {
        for status in statuses.into_iter().flatten() {
            match status {
                QuickbarItemMaterializationStatus::Proven(proof) => {
                    self.proven = self.proven.saturating_add(1);
                    match proof {
                        QuickbarItemMaterializationProof::ExplicitSelfMaterialization => {}
                        QuickbarItemMaterializationProof::ActiveObject => {
                            self.active_state = self.active_state.saturating_add(1);
                        }
                        QuickbarItemMaterializationProof::InventoryFeature25FirstList => {
                            self.feature25_first = self.feature25_first.saturating_add(1);
                        }
                        QuickbarItemMaterializationProof::InventoryFeature25SecondList => {
                            self.feature25_second = self.feature25_second.saturating_add(1);
                        }
                        QuickbarItemMaterializationProof::InventoryFeature25LegacyTail => {
                            self.feature25_legacy_tail =
                                self.feature25_legacy_tail.saturating_add(1);
                        }
                    }
                }
                QuickbarItemMaterializationStatus::Unknown => {
                    self.unknown = self.unknown.saturating_add(1);
                }
                QuickbarItemMaterializationStatus::ClearedByItemDelete => {
                    self.cleared_delete = self.cleared_delete.saturating_add(1);
                }
                QuickbarItemMaterializationStatus::ClearedByAreaReset => {
                    self.cleared_area_reset = self.cleared_area_reset.saturating_add(1);
                }
            }
        }
    }
}

#[derive(Default)]
struct QuickbarMaterializationCounts {
    explicit_self_materialization: u32,
    active_state: u32,
    feature25_first: u32,
    feature25_second: u32,
    feature25_legacy_tail: u32,
}

impl QuickbarMaterializationCounts {
    fn observe(&mut self, proofs: [Option<QuickbarItemMaterializationProof>; 2]) {
        for proof in proofs.into_iter().flatten() {
            match proof {
                QuickbarItemMaterializationProof::ExplicitSelfMaterialization => {
                    self.explicit_self_materialization =
                        self.explicit_self_materialization.saturating_add(1);
                }
                QuickbarItemMaterializationProof::ActiveObject => {
                    self.active_state = self.active_state.saturating_add(1);
                }
                QuickbarItemMaterializationProof::InventoryFeature25FirstList => {
                    self.feature25_first = self.feature25_first.saturating_add(1);
                }
                QuickbarItemMaterializationProof::InventoryFeature25SecondList => {
                    self.feature25_second = self.feature25_second.saturating_add(1);
                }
                QuickbarItemMaterializationProof::InventoryFeature25LegacyTail => {
                    self.feature25_legacy_tail = self.feature25_legacy_tail.saturating_add(1);
                }
            }
        }
    }
}

fn trace_quickbar_rewrite_skip(reason: &str, payload: &[u8]) {
    tracing::debug!(
        reason,
        payload_len = payload.len(),
        prefix = %hex_prefix(payload, 24),
        "server GuiQuickbar_SetAllButtons rewrite skipped"
    );
}

#[derive(Debug, Clone, Copy)]
enum QuickbarRewriteTraceRole {
    Committed,
    StreamProbe,
}

impl QuickbarRewriteTraceRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::Committed => "committed",
            Self::StreamProbe => "stream_probe",
        }
    }

    fn is_committed(self) -> bool {
        matches!(self, Self::Committed)
    }
}

fn trace_quickbar_rewrite_summary(
    path: &str,
    summary: &QuickbarRewriteSummary,
    trace_role: QuickbarRewriteTraceRole,
) {
    tracing::info!(
        path,
        trace_role = trace_role.as_str(),
        committed = trace_role.is_committed(),
        old_payload_length = summary.old_payload_length,
        new_payload_length = summary.new_payload_length,
        old_declared = summary.old_declared,
        new_declared = summary.new_declared,
        read_size = summary.read_size,
        fragment_size = summary.fragment_size,
        final_cursor = summary.final_cursor,
        trailing_read_bytes = summary.trailing_read_bytes,
        direct_opcode_stream = summary.direct_opcode_stream,
        slot_records_owned = summary.slot_records_owned,
        item_buttons_seen = summary.item_buttons_seen,
        item_buttons_source_explicit = summary.item_buttons_source_explicit,
        item_buttons_source_compact = summary.item_buttons_source_compact,
        item_buttons_source_recovered = summary.item_buttons_source_recovered,
        item_buttons_preserved = summary.item_buttons_preserved,
        spells_preserved = summary.spells_preserved,
        blank_buttons_seen = summary.blank_buttons_seen,
        general_buttons_preserved = summary.general_buttons_preserved,
        general_buttons_blanked = summary.general_buttons_blanked,
        item_buttons_blanked = summary.item_buttons_blanked,
        item_buttons_blanked_candidate = summary.item_buttons_blanked_candidate,
        unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
        item_buttons_rejected_recovered_type_tag = summary.item_buttons_rejected_recovered_type_tag,
        item_buttons_rejected_missing_type_source =
            summary.item_buttons_rejected_missing_type_source,
        item_buttons_rejected_no_present_item = summary.item_buttons_rejected_no_present_item,
        item_buttons_rejected_invalid_object_id = summary.item_buttons_rejected_invalid_object_id,
        item_buttons_rejected_missing_active_properties =
            summary.item_buttons_rejected_missing_active_properties,
        item_buttons_rejected_unsupported_appearance_type =
            summary.item_buttons_rejected_unsupported_appearance_type,
        item_buttons_rejected_appearance_shape = summary.item_buttons_rejected_appearance_shape,
        item_buttons_rejected_missing_state_proof =
            summary.item_buttons_rejected_missing_state_proof,
        item_buttons_rejected_missing_state_unknown =
            summary.item_buttons_rejected_missing_state_unknown,
        item_buttons_rejected_missing_state_cleared_delete =
            summary.item_buttons_rejected_missing_state_cleared_delete,
        item_buttons_rejected_missing_state_cleared_area_reset =
            summary.item_buttons_rejected_missing_state_cleared_area_reset,
        item_objects_rejected_missing_state_proven =
            summary.item_objects_rejected_missing_state_proven,
        item_objects_rejected_missing_state_active =
            summary.item_objects_rejected_missing_state_active,
        item_objects_rejected_missing_state_feature25_first =
            summary.item_objects_rejected_missing_state_feature25_first,
        item_objects_rejected_missing_state_feature25_second =
            summary.item_objects_rejected_missing_state_feature25_second,
        item_objects_rejected_missing_state_feature25_legacy_tail =
            summary.item_objects_rejected_missing_state_feature25_legacy_tail,
        item_objects_rejected_missing_state_unknown =
            summary.item_objects_rejected_missing_state_unknown,
        item_objects_rejected_missing_state_cleared_delete =
            summary.item_objects_rejected_missing_state_cleared_delete,
        item_objects_rejected_missing_state_cleared_area_reset =
            summary.item_objects_rejected_missing_state_cleared_area_reset,
        item_objects_preserved_by_explicit_self_materialization =
            summary.item_objects_preserved_by_explicit_self_materialization,
        item_objects_preserved_by_active_state = summary.item_objects_preserved_by_active_state,
        item_objects_preserved_by_feature25_first =
            summary.item_objects_preserved_by_feature25_first,
        item_objects_preserved_by_feature25_second =
            summary.item_objects_preserved_by_feature25_second,
        item_objects_preserved_by_feature25_legacy_tail =
            summary.item_objects_preserved_by_feature25_legacy_tail,
        "server GuiQuickbar_SetAllButtons rewrite summary"
    );
}

fn trace_quickbar_materialization_context(
    path: &str,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
    summary: &QuickbarRewriteSummary,
    trace_role: QuickbarRewriteTraceRole,
) {
    let Some(context) =
        materialization.and_then(|materialization| materialization.context_summary())
    else {
        return;
    };
    tracing::info!(
        path,
        trace_role = trace_role.as_str(),
        committed = trace_role.is_committed(),
        item_buttons_seen = summary.item_buttons_seen,
        item_buttons_preserved = summary.item_buttons_preserved,
        active_item_objects = context.active_item_objects,
        materialized_item_objects = context.materialized_item_objects,
        inventory_feature25_first_item_refs = context.inventory_feature25_first_item_refs,
        inventory_feature25_second_item_refs = context.inventory_feature25_second_item_refs,
        inventory_feature25_legacy_tail_item_refs =
            context.inventory_feature25_legacy_tail_item_refs,
        cleared_inventory_item_object_ids = context.cleared_inventory_item_object_ids,
        inventory_feature25_reference_records = context.inventory_feature25_reference_records,
        inventory_feature25_first_item_ref_mentions =
            context.inventory_feature25_first_item_ref_mentions,
        inventory_feature25_second_item_ref_mentions =
            context.inventory_feature25_second_item_ref_mentions,
        inventory_feature25_legacy_tail_item_ref_mentions =
            context.inventory_feature25_legacy_tail_item_ref_mentions,
        inventory_feature25_first_materialized_item_ref_mentions =
            context.inventory_feature25_first_materialized_item_ref_mentions,
        inventory_feature25_first_deferred_item_ref_mentions =
            context.inventory_feature25_first_deferred_item_ref_mentions,
        inventory_feature25_second_materialized_item_ref_mentions =
            context.inventory_feature25_second_materialized_item_ref_mentions,
        inventory_feature25_second_deferred_item_ref_mentions =
            context.inventory_feature25_second_deferred_item_ref_mentions,
        inventory_feature25_legacy_tail_materialized_item_ref_mentions =
            context.inventory_feature25_legacy_tail_materialized_item_ref_mentions,
        inventory_feature25_legacy_tail_deferred_item_ref_mentions =
            context.inventory_feature25_legacy_tail_deferred_item_ref_mentions,
        "server GuiQuickbar_SetAllButtons registry materialization context"
    );
}

fn trace_quickbar_item_button_decisions(
    path: &str,
    parsed: &QuickbarParse,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
    trace_role: QuickbarRewriteTraceRole,
) {
    for (slot_index, button) in parsed.buttons.iter().enumerate() {
        let QuickbarButtonKind::Item {
            primary,
            secondary,
            source,
            recovered_type_tag,
        } = &button.kind
        else {
            continue;
        };
        let decision = super::writer::quickbar_item_button_materialization_decision(
            primary,
            secondary,
            *source,
            *recovered_type_tag,
            materialization,
        );
        let accepted = decision.is_ok();
        let reject_reason = decision
            .as_ref()
            .err()
            .copied()
            .map(quickbar_item_reject_reason_label)
            .unwrap_or("none");
        let proofs = decision.ok();
        let primary_proof = proofs
            .as_ref()
            .and_then(|proofs| proofs[0])
            .map(quickbar_materialization_proof_label)
            .unwrap_or("none");
        let secondary_proof = proofs
            .as_ref()
            .and_then(|proofs| proofs[1])
            .map(quickbar_materialization_proof_label)
            .unwrap_or("none");
        let primary_status = quickbar_item_object_registry_status_label(primary, materialization);
        let secondary_status =
            quickbar_item_object_registry_status_label(secondary, materialization);
        let primary_shape = quickbar_item_object_shape_label(primary);
        let secondary_shape = quickbar_item_object_shape_label(secondary);
        let primary_active_property_count = primary
            .active_props
            .as_ref()
            .map(|active_props| active_props.properties.len())
            .unwrap_or(0);
        let secondary_active_property_count = secondary
            .active_props
            .as_ref()
            .map(|active_props| active_props.properties.len())
            .unwrap_or(0);
        tracing::info!(
            path,
            trace_role = trace_role.as_str(),
            committed = trace_role.is_committed(),
            slot_index,
            source = quickbar_item_source_label(*source),
            recovered_type_tag,
            accepted,
            reject_reason,
            primary_present = primary.present,
            primary_object_id = %format_args!("0x{:08X}", primary.object_id),
            primary_shape,
            primary_base_item = %format_args!("0x{:08X}", primary.base_item),
            primary_appearance_type = primary.appearance_type,
            primary_appearance_len = primary.appearance_bytes.len(),
            primary_active_properties_present = primary.active_props.is_some(),
            primary_active_property_count,
            primary_status,
            primary_proof,
            secondary_present = secondary.present,
            secondary_object_id = %format_args!("0x{:08X}", secondary.object_id),
            secondary_shape,
            secondary_base_item = %format_args!("0x{:08X}", secondary.base_item),
            secondary_appearance_type = secondary.appearance_type,
            secondary_appearance_len = secondary.appearance_bytes.len(),
            secondary_active_properties_present = secondary.active_props.is_some(),
            secondary_active_property_count,
            secondary_status,
            secondary_proof,
            "server GuiQuickbar_SetAllButtons item materialization decision"
        );
    }
}

fn quickbar_item_source_label(source: QuickbarItemSource) -> &'static str {
    match source {
        QuickbarItemSource::ExplicitTypeAndFragmentBits => "explicit_type_fragment_bits",
        QuickbarItemSource::CompactByteOwnedWithSourceType => "compact_byte_owned_source_type",
        QuickbarItemSource::RecoveredMissingType => "recovered_missing_type",
    }
}

fn quickbar_item_reject_reason_label(
    reason: super::writer::QuickbarItemMaterializationRejectReason,
) -> &'static str {
    match reason {
        super::writer::QuickbarItemMaterializationRejectReason::RecoveredTypeTag => {
            "recovered_type_tag"
        }
        super::writer::QuickbarItemMaterializationRejectReason::MissingTypeSource => {
            "missing_type_source"
        }
        super::writer::QuickbarItemMaterializationRejectReason::NoPresentItem => "no_present_item",
        super::writer::QuickbarItemMaterializationRejectReason::InvalidObjectId => {
            "invalid_object_id"
        }
        super::writer::QuickbarItemMaterializationRejectReason::MissingActiveProperties => {
            "missing_active_properties"
        }
        super::writer::QuickbarItemMaterializationRejectReason::UnsupportedAppearanceType => {
            "unsupported_appearance_type"
        }
        super::writer::QuickbarItemMaterializationRejectReason::AppearanceShape => {
            "appearance_shape"
        }
        super::writer::QuickbarItemMaterializationRejectReason::MissingStateProof => {
            "missing_state_proof"
        }
    }
}

fn quickbar_materialization_proof_label(proof: QuickbarItemMaterializationProof) -> &'static str {
    match proof {
        QuickbarItemMaterializationProof::ExplicitSelfMaterialization => {
            "explicit_self_materialization"
        }
        QuickbarItemMaterializationProof::ActiveObject => "active_object",
        QuickbarItemMaterializationProof::InventoryFeature25FirstList => {
            "inventory_feature25_first_list"
        }
        QuickbarItemMaterializationProof::InventoryFeature25SecondList => {
            "inventory_feature25_second_list"
        }
        QuickbarItemMaterializationProof::InventoryFeature25LegacyTail => {
            "inventory_feature25_legacy_tail"
        }
    }
}

fn quickbar_item_object_registry_status_label(
    item: &QuickbarItemObject,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> &'static str {
    if !item.present {
        return "absent";
    }
    if item.object_id == NWN_OBJECT_INVALID {
        return "invalid_object_id";
    }
    match materialization
        .map(|materialization| materialization.item_object_materialization_status(item.object_id))
        .unwrap_or(QuickbarItemMaterializationStatus::Unknown)
    {
        QuickbarItemMaterializationStatus::Proven(proof) => {
            quickbar_materialization_proof_label(proof)
        }
        QuickbarItemMaterializationStatus::ClearedByItemDelete => "cleared_by_item_delete",
        QuickbarItemMaterializationStatus::ClearedByAreaReset => "cleared_by_area_reset",
        QuickbarItemMaterializationStatus::Unknown => "unknown",
    }
}

fn quickbar_item_object_shape_label(item: &QuickbarItemObject) -> &'static str {
    match super::writer::quickbar_item_object_shape_status(item) {
        super::writer::QuickbarItemObjectShapeStatus::Absent => "absent",
        super::writer::QuickbarItemObjectShapeStatus::InvalidObjectId => "invalid_object_id",
        super::writer::QuickbarItemObjectShapeStatus::MissingActiveProperties => {
            "missing_active_properties"
        }
        super::writer::QuickbarItemObjectShapeStatus::UnsupportedAppearanceType => {
            "unsupported_appearance_type"
        }
        super::writer::QuickbarItemObjectShapeStatus::AppearanceShape => "appearance_shape",
        super::writer::QuickbarItemObjectShapeStatus::Valid => "valid",
    }
}

fn dump_quickbar_payload(label: &str, payload: &[u8]) {
    let Ok(enabled) = std::env::var("NWN_BRIDGE_DUMP_QUICKBAR") else {
        return;
    };
    if enabled != "1" && enabled.to_ascii_lowercase() != "true" {
        return;
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    let filename = format!("quickbar_{label}_{millis}.bin");
    let path = std::env::var("NWN_BRIDGE_QUICKBAR_DUMP_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(filename);
    if let Some(parent) = path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            tracing::warn!(%error, path = %parent.display(), "failed to create quickbar dump directory");
            return;
        }
    }
    if let Err(error) = fs::write(&path, payload) {
        tracing::warn!(%error, path = %path.display(), "failed to dump quickbar payload");
    }
}

fn hex_prefix(bytes: &[u8], max: usize) -> String {
    bytes
        .iter()
        .take(max)
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shield_item(object_id: u32) -> QuickbarItemObject {
        let base_item = 0x38_u32;
        let mut appearance_bytes = Vec::new();
        appearance_bytes.extend_from_slice(&base_item.to_le_bytes());
        appearance_bytes.push(0x34);
        QuickbarItemObject {
            present: true,
            object_id,
            int_param: -1,
            base_item,
            appearance_type: 0,
            active_props: Some(QuickbarActiveItemProperties::default()),
            appearance_bytes,
        }
    }

    #[test]
    fn summary_counts_item_object_statuses_on_missing_state_rejects() {
        let primary = shield_item(0x8000_0101);
        let secondary = shield_item(0x8000_0102);
        let primary_object_id = primary.object_id;
        let secondary_object_id = secondary.object_id;
        let materialization_status = |object_id| {
            if object_id == primary_object_id {
                QuickbarItemMaterializationStatus::Proven(
                    QuickbarItemMaterializationProof::InventoryFeature25FirstList,
                )
            } else if object_id == secondary_object_id {
                QuickbarItemMaterializationStatus::ClearedByAreaReset
            } else {
                QuickbarItemMaterializationStatus::Unknown
            }
        };
        let materialization =
            QuickbarMaterializationContext::new_with_status(&materialization_status);
        let parsed = QuickbarParse {
            envelope: b'P',
            declared: 0,
            read_size: 0,
            fragment_size: 1,
            final_cursor: 0,
            buttons: vec![QuickbarButton {
                kind: QuickbarButtonKind::Item {
                    primary,
                    secondary,
                    source: QuickbarItemSource::CompactByteOwnedWithSourceType,
                    recovered_type_tag: false,
                },
            }],
            direct_opcode_stream: false,
        };

        let summary = summarize_quickbar_rewrite(&parsed, 0, 0, 0, 0, Some(&materialization));

        assert_eq!(summary.item_buttons_seen, 1);
        assert_eq!(summary.item_buttons_preserved, 0);
        assert_eq!(summary.item_buttons_rejected_missing_state_proof, 1);
        assert_eq!(
            summary.item_buttons_rejected_missing_state_cleared_area_reset,
            1
        );
        assert_eq!(summary.item_objects_rejected_missing_state_proven, 1);
        assert_eq!(
            summary.item_objects_rejected_missing_state_feature25_first,
            1
        );
        assert_eq!(
            summary.item_objects_rejected_missing_state_cleared_area_reset,
            1
        );
        assert_eq!(summary.item_objects_rejected_missing_state_unknown, 0);
    }
}
