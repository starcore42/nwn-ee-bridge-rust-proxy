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
            "2".to_string(),
        ],
    );
    write_tsv_line(
        &mut out,
        &[
            "ownership".to_string(),
            "status".to_string(),
            "unproven-source-owner".to_string(),
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
            "payload_prefix".to_string(),
            crate::packet::hex_prefix(payload, 64),
        ],
    );
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
                "residual".to_string(),
                format_rewrite_bit_slice_evidence(stock.source_reader_residual),
                "claimable".to_string(),
                "false".to_string(),
            ],
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
