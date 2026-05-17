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

pub fn rewrite_simple_quickbar_payload_with_context_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
) -> Option<QuickbarRewriteSummary> {
    let old_payload_length = payload.len();
    let parsed = parse_cnw_quickbar_payload(payload)?;
    dump_quickbar_payload("simple_before", payload);
    let old_declared = parsed.declared;
    let rewritten = match super::writer::build_ee_quickbar_payload_with_context(
        &parsed,
        materialization,
    ) {
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
    trace_quickbar_rewrite_summary("simple", &summary);
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

pub fn normalize_and_rewrite_quickbar_payload_with_context_if_possible(
    payload: &mut Vec<u8>,
    materialization: Option<&QuickbarMaterializationContext<'_>>,
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
    trace_quickbar_rewrite_summary("normalized", &summary);
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
        && (summary.spells_preserved != 0
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
    parse_cnw_quickbar_payload_with_ee_declared(payload, high, declared, declared_usize)
        .or_else(|| {
            parse_cnw_quickbar_payload_with_legacy_short_declared(
                payload,
                high,
                declared,
                declared_usize,
            )
        })
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
    let (buttons, final_cursor) = (if fragment_size != 0 {
        parse_quickbar_read_buffer_with_fragments(read_buffer, fragments, cursor)
    } else {
        parse_quickbar_read_buffer(read_buffer, cursor)
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
    let item_buttons_emitted = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                &button.kind,
                QuickbarButtonKind::Item {
                    primary,
                    secondary,
                    source,
                    recovered_type_tag,
                } if super::writer::quickbar_item_button_has_verified_ee_materialization(
                    primary,
                    secondary,
                    *source,
                    *recovered_type_tag,
                    materialization,
                )
            )
        })
        .count() as u32;
    let item_buttons_blanked_by_policy = parsed
        .buttons
        .iter()
        .filter(|button| {
            matches!(
                &button.kind,
                QuickbarButtonKind::Item {
                    primary,
                    secondary,
                    source,
                    recovered_type_tag,
                } if !super::writer::quickbar_item_button_has_verified_ee_materialization(
                    primary,
                    secondary,
                    *source,
                    *recovered_type_tag,
                    materialization,
                )
            )
        })
        .count() as u32;
    let spells_preserved = parsed
        .buttons
        .iter()
        .filter(|button| matches!(button.kind, QuickbarButtonKind::Spell { .. }))
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
        item_buttons_preserved: item_buttons_emitted,
        spells_preserved,
        general_buttons_preserved,
        general_buttons_blanked,
        item_buttons_blanked: item_candidate_buttons.saturating_add(item_buttons_blanked_by_policy),
        unsupported_buttons_blanked,
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

fn trace_quickbar_rewrite_summary(path: &str, summary: &QuickbarRewriteSummary) {
    tracing::info!(
        path,
        old_payload_length = summary.old_payload_length,
        new_payload_length = summary.new_payload_length,
        old_declared = summary.old_declared,
        new_declared = summary.new_declared,
        read_size = summary.read_size,
        fragment_size = summary.fragment_size,
        final_cursor = summary.final_cursor,
        trailing_read_bytes = summary.trailing_read_bytes,
        direct_opcode_stream = summary.direct_opcode_stream,
        item_buttons_preserved = summary.item_buttons_preserved,
        spells_preserved = summary.spells_preserved,
        general_buttons_preserved = summary.general_buttons_preserved,
        general_buttons_blanked = summary.general_buttons_blanked,
        item_buttons_blanked = summary.item_buttons_blanked,
        unsupported_buttons_blanked = summary.unsupported_buttons_blanked,
        "server GuiQuickbar_SetAllButtons rewrite summary"
    );
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
