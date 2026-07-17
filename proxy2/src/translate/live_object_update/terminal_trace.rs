use super::{
    LiveObjectUpdateRewriteFailure, format_live_object_byte, format_optional_bool,
    format_optional_u32_hex, format_optional_usize, format_rewrite_bit_slice_evidence,
    write_rewrite_tail_capture, write_tsv_line,
};

/// Format bounded terminal door/placeable fragment evidence for comparison
/// with a source-writer trace.
///
/// This deliberately serializes only the typed failure evidence retained by
/// the exact parser and rewrite ledger. End-aligned readers, repeated prior
/// spans, and compact suffix candidates remain interpretations of immutable
/// source bits: no row in this artifact authorizes a rewrite, cursor advance,
/// or fragment trim.
pub(crate) fn format_live_object_update_terminal_tail9_handoff_capture(
    source: &str,
    payload: &[u8],
    failure: LiveObjectUpdateRewriteFailure,
) -> Option<String> {
    let evidence = failure.terminal_door_placeable_tail9_residual?;
    let mut out = String::new();

    write_tsv_line(
        &mut out,
        &[
            "capture".to_string(),
            "live-object-terminal-tail9-handoff".to_string(),
            "version".to_string(),
            "6".to_string(),
        ],
    );
    write_tsv_line(
        &mut out,
        &[
            "ownership".to_string(),
            "status".to_string(),
            "unproven-source-owner".to_string(),
            "source_fragment_ownership".to_string(),
            evidence
                .source_fragment_ownership_verdict()
                .as_str()
                .to_string(),
            "emitted_fragment_ownership".to_string(),
            evidence
                .emitted_fragment_ownership_verdict()
                .as_str()
                .to_string(),
            "claimable".to_string(),
            "false".to_string(),
            "rewrite_authorized".to_string(),
            "false".to_string(),
            "cursor_advance_authorized".to_string(),
            "false".to_string(),
            "fragment_trim_authorized".to_string(),
            "false".to_string(),
            "required_proof".to_string(),
            "source-writer-or-list-handoff".to_string(),
        ],
    );
    write_tsv_line(
        &mut out,
        &[
            "meta".to_string(),
            "source".to_string(),
            source.to_string(),
            "payload_len".to_string(),
            payload.len().to_string(),
            "payload_md5_hint".to_string(),
            format!("{:x}", md5::compute(payload)),
            "payload_prefix".to_string(),
            crate::packet::hex_prefix(payload, 64),
        ],
    );
    if let Some(requirement) = evidence.writer_handoff_requirement() {
        write_tsv_line(
            &mut out,
            &[
                "writer_handoff_requirement".to_string(),
                "object_type".to_string(),
                format_live_object_byte(requirement.object_type),
                "object_id".to_string(),
                format_optional_u32_hex(Some(requirement.object_id)),
                "raw_mask".to_string(),
                format!("0x{:08X}", requirement.raw_mask),
                "source_read_buffer".to_string(),
                format!(
                    "{}..{}",
                    requirement.source_read_buffer_cursor, requirement.source_read_buffer_end
                ),
                "source_fragment".to_string(),
                format_rewrite_bit_slice_evidence(requirement.source_fragment_bits),
                "source_next_opcode_read_overflows".to_string(),
                requirement.source_next_opcode_read_overflows.to_string(),
                "emitted_read_buffer".to_string(),
                format!(
                    "{}..{}",
                    requirement.emitted_read_buffer_cursor, requirement.emitted_read_buffer_end
                ),
                "emitted_fragment_obligation".to_string(),
                format!(
                    "{}..{}",
                    requirement.emitted_fragment_bit_start, requirement.emitted_fragment_bit_end
                ),
                "emitted_fragment_bits".to_string(),
                requirement.emitted_fragment_bit_count.to_string(),
                "emitted_fragment_bits_retained".to_string(),
                requirement.emitted_fragment_bits_retained.to_string(),
                "emitted_fragment_preview".to_string(),
                format_rewrite_bit_slice_evidence(evidence.rewritten_residual),
                "emitted_fragment_values_complete".to_string(),
                (requirement.emitted_fragment_bits_retained
                    == requirement.emitted_fragment_bit_count)
                    .to_string(),
                "emitted_next_opcode_read_overflows".to_string(),
                requirement.emitted_next_opcode_read_overflows.to_string(),
                "packet_correlation_required".to_string(),
                "exact-payload-bytes".to_string(),
                "final_ee_claim_required".to_string(),
                "true".to_string(),
                "claimable".to_string(),
                "false".to_string(),
                "rewrite_authorized".to_string(),
                "false".to_string(),
                "fragment_trim_authorized".to_string(),
                "false".to_string(),
            ],
        );
        let unavailable = requirement.correlate(None);
        write_tsv_line(
            &mut out,
            &[
                "writer_handoff_correlation".to_string(),
                "observation".to_string(),
                "none".to_string(),
                "verdict".to_string(),
                unavailable.as_str().to_string(),
                "writer_handoff_observed".to_string(),
                unavailable.writer_handoff_observed().to_string(),
                "claimable".to_string(),
                unavailable.allows_exact_claim().to_string(),
            ],
        );
    } else {
        write_tsv_line(
            &mut out,
            &[
                "writer_handoff_requirement".to_string(),
                "unavailable-incomplete-bounded-evidence".to_string(),
                "claimable".to_string(),
                "false".to_string(),
            ],
        );
    }
    write_tsv_line(
        &mut out,
        &[
            "failure".to_string(),
            "reason".to_string(),
            failure.reason.to_string(),
            "kind".to_string(),
            failure.kind.as_str().to_string(),
            "offset".to_string(),
            failure.offset.to_string(),
            "record_end".to_string(),
            failure.record_end.to_string(),
            "bit_cursor".to_string(),
            failure.bit_cursor.to_string(),
        ],
    );
    write_tsv_line(
        &mut out,
        &[
            "selected_source_reader".to_string(),
            "raw_mask".to_string(),
            format!("0x{:08X}", evidence.raw_mask),
            "translated_mask".to_string(),
            format!("0x{:08X}", evidence.translated_mask),
            "fragment_bits".to_string(),
            evidence.source_fragment_bit_count.to_string(),
            "start".to_string(),
            evidence.source_bit_cursor.to_string(),
            "end".to_string(),
            evidence.source_reader_bit_cursor.to_string(),
            "consumed".to_string(),
            evidence.source_reader_bits_consumed.to_string(),
            "name_selector".to_string(),
            format_optional_usize(evidence.source_name_selector_bit_cursor),
            "name_selector_value".to_string(),
            format_optional_bool(evidence.source_name_selector),
            "locstring_selector".to_string(),
            format_optional_usize(evidence.source_name_locstring_selector_bit_cursor),
            "locstring_selector_value".to_string(),
            format_optional_bool(evidence.source_name_locstring_selector),
            "name_kind".to_string(),
            evidence.source_name_kind.unwrap_or("none").to_string(),
            "residual".to_string(),
            format_rewrite_bit_slice_evidence(evidence.source_reader_residual),
        ],
    );
    write_tsv_line(
        &mut out,
        &[
            "emitted_reader".to_string(),
            "start".to_string(),
            evidence.emitted_bit_cursor.to_string(),
            "fragment_bits".to_string(),
            evidence.emitted_fragment_bit_count.to_string(),
            "rewritten_end".to_string(),
            evidence.rewritten_bit_cursor.to_string(),
            "rewritten_fragment_bits".to_string(),
            evidence.rewritten_fragment_bit_count.to_string(),
            "residual_bits".to_string(),
            evidence.residual_fragment_bits.to_string(),
            "residual".to_string(),
            format_rewrite_bit_slice_evidence(evidence.rewritten_residual),
            "proven_packed_name_bits".to_string(),
            evidence.proven_terminal_packed_name_bits.to_string(),
        ],
    );

    if let Some(stock) = evidence.stock_diamond_source {
        write_tsv_line(
            &mut out,
            &[
                "stock_diamond_reader".to_string(),
                "raw_mask".to_string(),
                format!("0x{:08X}", stock.raw_mask),
                "effective_mask".to_string(),
                format!("0x{:08X}", stock.effective_mask),
                "ignored_mask".to_string(),
                format!("0x{:08X}", stock.ignored_mask),
                "read_end".to_string(),
                stock.read_end.to_string(),
                "start".to_string(),
                stock.source_bit_cursor.to_string(),
                "end".to_string(),
                stock.source_reader_bit_cursor.to_string(),
                "consumed".to_string(),
                stock.source_reader_bits_consumed.to_string(),
                "orientation_vector".to_string(),
                format_optional_bool(stock.source_orientation_vector),
                "state_cursor".to_string(),
                format_optional_usize(stock.source_state_bit_cursor),
                "name_selector".to_string(),
                format_optional_usize(stock.source_name_selector_bit_cursor),
                "name_selector_value".to_string(),
                format_optional_bool(stock.source_name_selector),
                "locstring_selector".to_string(),
                format_optional_usize(stock.source_name_locstring_selector_bit_cursor),
                "locstring_selector_value".to_string(),
                format_optional_bool(stock.source_name_locstring_selector),
                "name_kind".to_string(),
                stock.source_name_kind.unwrap_or("none").to_string(),
                "object_type".to_string(),
                format_live_object_byte(evidence.object_type),
                "object_id".to_string(),
                format_optional_u32_hex(Some(evidence.object_id)),
                "residual".to_string(),
                format_rewrite_bit_slice_evidence(stock.source_reader_residual),
                "claimable".to_string(),
                "false".to_string(),
            ],
        );
        write_diamond_fragment_field_rows(
            &mut out,
            "stock_diamond_fragment_field",
            None,
            stock.raw_mask,
            stock.effective_mask,
            stock.source_bit_cursor,
            stock.source_orientation_vector,
            stock.source_state_bit_cursor,
            stock.source_name_selector_bit_cursor,
            stock.source_name_locstring_selector_bit_cursor,
            evidence.object_type,
            evidence.object_id,
            |bit_cursor| stock.source_reader_bits.bit(bit_cursor),
        );
    } else {
        write_tsv_line(
            &mut out,
            &["stock_diamond_reader".to_string(), "none".to_string()],
        );
    }

    let continuation = evidence.terminal_reader_continuation;
    write_tsv_line(
        &mut out,
        &[
            "reader_continuation".to_string(),
            "source_read_buffer".to_string(),
            format!(
                "{}..{}",
                continuation.source_read_buffer_cursor, continuation.source_read_buffer_end
            ),
            "source_fragment".to_string(),
            format!(
                "{}..{}",
                continuation.source_fragment_bit_cursor, continuation.source_fragment_bit_end
            ),
            "source_more_data".to_string(),
            continuation.source_more_data_source.as_str().to_string(),
            "source_next_opcode_read_overflows".to_string(),
            continuation.source_next_opcode_read_overflows.to_string(),
            "emitted_read_buffer".to_string(),
            format!(
                "{}..{}",
                continuation.emitted_read_buffer_cursor, continuation.emitted_read_buffer_end
            ),
            "emitted_fragment".to_string(),
            format!(
                "{}..{}",
                continuation.emitted_fragment_bit_cursor, continuation.emitted_fragment_bit_end
            ),
            "emitted_more_data".to_string(),
            continuation.emitted_more_data_source.as_str().to_string(),
            "emitted_next_opcode_read_overflows".to_string(),
            continuation.emitted_next_opcode_read_overflows.to_string(),
            "claimable".to_string(),
            "false".to_string(),
        ],
    );

    let reused_record_interpretation = evidence.reused_record_reader_interpretation();
    let reused_record_count = usize::from(reused_record_interpretation.is_some());
    write_tsv_line(
        &mut out,
        &[
            "reused_record_reader_summary".to_string(),
            "candidates".to_string(),
            reused_record_count.to_string(),
            "retained".to_string(),
            reused_record_count.to_string(),
            "ownership".to_string(),
            "unknown".to_string(),
            "claimable".to_string(),
            "false".to_string(),
        ],
    );
    if let Some(candidate) = reused_record_interpretation {
        write_tsv_line(
            &mut out,
            &[
                "reused_record_reader_interpretation".to_string(),
                candidate.candidate_index.to_string(),
                "dialect".to_string(),
                "diamond".to_string(),
                "record_end".to_string(),
                candidate.record_end.to_string(),
                "read_buffer".to_string(),
                format!(
                    "{}..{}",
                    candidate.read_buffer_cursor, candidate.read_buffer_end
                ),
                "required_second_row_header_bytes".to_string(),
                candidate.required_second_row_header_bytes.to_string(),
                "available_second_row_header_bytes".to_string(),
                candidate.available_second_row_header_bytes.to_string(),
                "stock_fragment".to_string(),
                format!(
                    "{}..{}",
                    candidate.stock_fragment_bit_start, candidate.stock_fragment_bit_end
                ),
                "candidate_fragment".to_string(),
                format!(
                    "{}..{}",
                    candidate.candidate_fragment_bit_start, candidate.candidate_fragment_bit_end
                ),
                "fragment_gap_bits".to_string(),
                candidate.fragment_gap_bits.to_string(),
                "reader_shape_bits".to_string(),
                candidate.reader_shape_bits.to_string(),
                "same_ordered_field_topology".to_string(),
                "true".to_string(),
                "second_stock_row_dispatch_possible".to_string(),
                candidate.second_stock_row_dispatch_possible.to_string(),
                "writer_replay_proven".to_string(),
                "false".to_string(),
                "claimable".to_string(),
                "false".to_string(),
                "rewrite_authorized".to_string(),
                "false".to_string(),
                "fragment_trim_authorized".to_string(),
                "false".to_string(),
            ],
        );
    }

    if let Some(handoff) = evidence.terminal_fragment_handoff_correlation {
        write_tsv_line(
            &mut out,
            &[
                "terminal_handoff".to_string(),
                "anchored_cursor".to_string(),
                handoff.anchored_source_bit_cursor.to_string(),
                "fragment_bits".to_string(),
                handoff.source_fragment_bit_count.to_string(),
                "unresolved".to_string(),
                format_rewrite_bit_slice_evidence(handoff.unresolved_source_bits),
                "candidates".to_string(),
                handoff.candidate_count.to_string(),
                "retained".to_string(),
                handoff.candidates_retained.to_string(),
                "ambiguity".to_string(),
                handoff.ambiguity_count.to_string(),
                "claimable".to_string(),
                "false".to_string(),
            ],
        );
        for (index, candidate) in handoff.candidates.iter().flatten().enumerate() {
            write_tsv_line(
                &mut out,
                &[
                    "terminal_replay_candidate".to_string(),
                    index.to_string(),
                    "prior".to_string(),
                    format!("{}..{}", candidate.prior_offset, candidate.prior_record_end),
                    "opcode".to_string(),
                    format_live_object_byte(candidate.prior_opcode),
                    "marker".to_string(),
                    format_live_object_byte(candidate.prior_marker),
                    "prior_object_id".to_string(),
                    format_optional_u32_hex(candidate.prior_object_id),
                    "focus".to_string(),
                    candidate.focus_offset.to_string(),
                    "focus_object_id".to_string(),
                    format_optional_u32_hex(candidate.focus_object_id),
                    "same_object".to_string(),
                    candidate.same_object.to_string(),
                    "immediately_precedes".to_string(),
                    candidate.immediately_precedes_focus.to_string(),
                    "prior_source".to_string(),
                    format!(
                        "{}..{}",
                        candidate.prior_source_bit_start, candidate.prior_source_bit_end
                    ),
                    "prior_source_bits".to_string(),
                    candidate.prior_source_bit_count.to_string(),
                    "unresolved_prefix".to_string(),
                    format_rewrite_bit_slice_evidence(candidate.unresolved_prefix),
                    "replayed".to_string(),
                    format_rewrite_bit_slice_evidence(candidate.replayed_source_bits),
                    "unresolved_suffix".to_string(),
                    format_rewrite_bit_slice_evidence(candidate.unresolved_suffix),
                    "claimable".to_string(),
                    "false".to_string(),
                ],
            );
            if let Some(replay) = candidate.direct_name_placeable_add_replay {
                write_tsv_line(
                    &mut out,
                    &[
                        "terminal_semantic_replay".to_string(),
                        index.to_string(),
                        "kind".to_string(),
                        "direct-name-placeable-add".to_string(),
                        "source_name_selector".to_string(),
                        replay.source_name_selector_bit_cursor.to_string(),
                        "emitted_name_selector".to_string(),
                        replay.emitted_name_selector_bit_cursor.to_string(),
                        "prior_emitted".to_string(),
                        format!(
                            "{}..{}",
                            replay.prior_emitted_bit_start, replay.prior_emitted_bit_end
                        ),
                        "prior_emitted_bits".to_string(),
                        replay.prior_emitted_bit_count.to_string(),
                        "inserted".to_string(),
                        replay.prior_bits_inserted.to_string(),
                        "removed".to_string(),
                        replay.prior_bits_removed.to_string(),
                        "post_name".to_string(),
                        replay.emitted_post_name_bit_cursor.to_string(),
                        "next".to_string(),
                        replay.emitted_next_bit_cursor.to_string(),
                        "emitted".to_string(),
                        format_rewrite_bit_slice_evidence(replay.emitted_bits),
                        "claimable".to_string(),
                        "false".to_string(),
                        "rewrite_authorized".to_string(),
                        "false".to_string(),
                    ],
                );
            }
        }
    } else {
        write_tsv_line(
            &mut out,
            &["terminal_handoff".to_string(), "none".to_string()],
        );
    }

    let end_aligned_retained = evidence
        .end_aligned_diamond_reader_candidates
        .iter()
        .flatten()
        .count();
    write_tsv_line(
        &mut out,
        &[
            "end_aligned_summary".to_string(),
            "candidates".to_string(),
            evidence
                .end_aligned_diamond_reader_candidate_count
                .to_string(),
            "retained".to_string(),
            end_aligned_retained.to_string(),
            "claimable".to_string(),
            "false".to_string(),
        ],
    );
    for (index, candidate) in evidence
        .end_aligned_diamond_reader_candidates
        .iter()
        .flatten()
        .enumerate()
    {
        write_tsv_line(
            &mut out,
            &[
                "end_aligned_candidate".to_string(),
                index.to_string(),
                "raw_mask".to_string(),
                format!("0x{:08X}", candidate.raw_mask),
                "effective_mask".to_string(),
                format!("0x{:08X}", candidate.effective_mask),
                "ignored_mask".to_string(),
                format!("0x{:08X}", candidate.ignored_mask),
                "read_end".to_string(),
                candidate.read_end.to_string(),
                "start".to_string(),
                candidate.source_bit_cursor.to_string(),
                "end".to_string(),
                candidate.source_reader_bit_cursor.to_string(),
                "consumed".to_string(),
                candidate.source_reader_bits_consumed.to_string(),
                "orientation_vector".to_string(),
                format_optional_bool(candidate.source_orientation_vector),
                "state_cursor".to_string(),
                format_optional_usize(candidate.source_state_bit_cursor),
                "name_selector".to_string(),
                format_optional_usize(candidate.source_name_selector_bit_cursor),
                "name_selector_value".to_string(),
                format_optional_bool(candidate.source_name_selector),
                "name_kind".to_string(),
                candidate.source_name_kind.unwrap_or("none").to_string(),
                "object_type".to_string(),
                format_live_object_byte(evidence.object_type),
                "object_id".to_string(),
                format_optional_u32_hex(Some(evidence.object_id)),
                "gap_from_ledger".to_string(),
                format_rewrite_bit_slice_evidence(candidate.source_gap_from_ledger_cursor),
                "gap_from_anchor".to_string(),
                candidate
                    .source_gap_from_anchored_reader
                    .map(format_rewrite_bit_slice_evidence)
                    .unwrap_or_else(|| "none".to_string()),
                "source_bits".to_string(),
                format_rewrite_bit_slice_evidence(candidate.source_bits),
                "claimable".to_string(),
                "false".to_string(),
            ],
        );
        write_diamond_fragment_field_rows(
            &mut out,
            "end_aligned_fragment_field",
            Some(index),
            candidate.raw_mask,
            candidate.effective_mask,
            candidate.source_bit_cursor,
            candidate.source_orientation_vector,
            candidate.source_state_bit_cursor,
            candidate.source_name_selector_bit_cursor,
            candidate.source_name_locstring_selector_bit_cursor,
            evidence.object_type,
            evidence.object_id,
            |bit_cursor| {
                let offset = bit_cursor.checked_sub(candidate.source_bits.bit_start)?;
                candidate.source_bits.bits.get(offset).copied().flatten()
            },
        );
    }

    let compact_suffix_retained = evidence.source_suffix_candidates.iter().flatten().count();
    write_tsv_line(
        &mut out,
        &[
            "compact_suffix_summary".to_string(),
            "candidates".to_string(),
            evidence.source_suffix_candidate_count.to_string(),
            "retained".to_string(),
            compact_suffix_retained.to_string(),
            "claimable".to_string(),
            "false".to_string(),
        ],
    );
    for (index, candidate) in evidence
        .source_suffix_candidates
        .iter()
        .flatten()
        .enumerate()
    {
        write_tsv_line(
            &mut out,
            &[
                "compact_suffix_candidate".to_string(),
                index.to_string(),
                "start".to_string(),
                candidate.source_bit_cursor.to_string(),
                "end".to_string(),
                candidate.source_reader_bit_cursor.to_string(),
                "consumed".to_string(),
                candidate.source_reader_bits_consumed.to_string(),
                "name_selector".to_string(),
                format_optional_usize(candidate.source_name_selector_bit_cursor),
                "name_selector_value".to_string(),
                format_optional_bool(candidate.source_name_selector),
                "locstring_selector".to_string(),
                format_optional_usize(candidate.source_name_locstring_selector_bit_cursor),
                "locstring_selector_value".to_string(),
                format_optional_bool(candidate.source_name_locstring_selector),
                "name_kind".to_string(),
                candidate.source_name_kind.unwrap_or("none").to_string(),
                "gap_from_selected".to_string(),
                format_rewrite_bit_slice_evidence(candidate.source_gap_from_selected_reader),
                "source_bits".to_string(),
                format_rewrite_bit_slice_evidence(candidate.source_bits),
                "claimable".to_string(),
                "false".to_string(),
            ],
        );
    }

    write_rewrite_tail_capture(&mut out, evidence.precursor_tail);
    Some(out)
}

#[allow(clippy::too_many_arguments)]
fn write_diamond_fragment_field_rows<F>(
    out: &mut String,
    row_kind: &str,
    candidate_index: Option<usize>,
    raw_mask: u32,
    effective_mask: u32,
    source_bit_cursor: usize,
    orientation_vector: Option<bool>,
    state_bit_cursor: Option<usize>,
    name_selector_bit_cursor: Option<usize>,
    name_locstring_selector_bit_cursor: Option<usize>,
    object_type: u8,
    object_id: u32,
    mut source_bit: F,
) where
    F: FnMut(usize) -> Option<bool>,
{
    use super::LiveObjectUpdateDoorPlaceableFragmentFieldKind as Kind;

    let mut field_index = 0usize;
    let mut emit = |kind: Kind, bit_start: usize, bit_end: usize| {
        let mut columns = Vec::with_capacity(24);
        columns.push(row_kind.to_string());
        if let Some(index) = candidate_index {
            columns.push(index.to_string());
            columns.push("field".to_string());
        }
        columns.push(field_index.to_string());
        columns.push("kind".to_string());
        columns.push(kind.as_str().to_string());
        columns.push("dialect".to_string());
        columns.push("diamond".to_string());
        columns.push("object_type".to_string());
        columns.push(format_live_object_byte(object_type));
        columns.push("object_id".to_string());
        columns.push(format_optional_u32_hex(Some(object_id)));
        columns.push("mask".to_string());
        columns.push(format!("0x{raw_mask:08X}"));
        columns.push("source".to_string());
        let mut source = format!("{bit_start}..{bit_end}:");
        for bit_cursor in bit_start..bit_end {
            source.push(match source_bit(bit_cursor) {
                Some(false) => '0',
                Some(true) => '1',
                None => '?',
            });
        }
        columns.push(source);
        columns.push("probe_cursor".to_string());
        // DiamondFragmentCursor uses the full fragment vector: the three CNW
        // header bits are already present before the typed writer begins.
        columns.push(format!("{bit_start}..{bit_end}"));
        columns.push("claimable".to_string());
        columns.push("false".to_string());
        columns.push("rewrite_authorized".to_string());
        columns.push("false".to_string());
        write_tsv_line(out, &columns);
        field_index = field_index.saturating_add(1);
    };

    let mut cursor = source_bit_cursor;
    if (effective_mask & super::LEGACY_UPDATE_POSITION_MASK) != 0 {
        emit(Kind::PositionZLow, cursor, cursor.saturating_add(2));
        cursor = cursor.saturating_add(2);
    }
    if (effective_mask & super::LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        if let Some(vector) = orientation_vector {
            emit(Kind::OrientationSelector, cursor, cursor.saturating_add(1));
            if !vector {
                emit(
                    Kind::ScalarOrientationLow,
                    cursor.saturating_add(1),
                    cursor.saturating_add(5),
                );
            }
        }
    }
    if (effective_mask & super::LEGACY_UPDATE_STATE_MASK) != 0 {
        if let Some(state_start) = state_bit_cursor {
            for (offset, kind) in [
                Kind::StateVisualSelector,
                Kind::StateVisualActive,
                Kind::StateLocked,
                Kind::StateLockable,
                Kind::StateVisualPayload,
            ]
            .into_iter()
            .enumerate()
            {
                let bit_start = state_start.saturating_add(offset);
                emit(kind, bit_start, bit_start.saturating_add(1));
            }
        }
    }
    if (effective_mask & super::LEGACY_UPDATE_NAME_MASK) != 0 {
        if let Some(bit_start) = name_selector_bit_cursor {
            emit(Kind::NameSelector, bit_start, bit_start.saturating_add(1));
        }
        if let Some(bit_start) = name_locstring_selector_bit_cursor {
            emit(
                Kind::NameLocStringSelector,
                bit_start,
                bit_start.saturating_add(1),
            );
        }
    }

    debug_assert!(
        field_index <= super::LIVE_OBJECT_UPDATE_DOOR_PLACEABLE_FRAGMENT_FIELD_LIMIT,
        "exact Diamond fragment field walk must stay within its bounded schema"
    );
}
