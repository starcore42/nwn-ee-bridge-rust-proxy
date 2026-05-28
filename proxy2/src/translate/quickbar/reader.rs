use super::*;

// Decompile-owned 36-slot quickbar reader and non-item button parser.

pub(super) fn parse_direct_opcode_quickbar_stream(payload: &[u8]) -> Option<QuickbarParse> {
    if payload.len() <= HIGH_LEVEL_HEADER_BYTES {
        return None;
    }
    let high = HighLevel::parse(payload)?;
    if high.major != QUICKBAR_MAJOR || high.minor != SET_ALL_BUTTONS_MINOR {
        return None;
    }

    let read_buffer = &payload[HIGH_LEVEL_HEADER_BYTES..];
    let (buttons, final_cursor) = parse_quickbar_read_buffer(read_buffer, 0)?;
    if final_cursor != read_buffer.len() {
        return None;
    }
    let has_non_empty_content = buttons.iter().any(|button| {
        !matches!(
            button.kind,
            QuickbarButtonKind::General { ref bytes } if bytes.len() == 1 && bytes[0] == 0
        )
    });
    if !has_non_empty_content {
        return None;
    }

    Some(QuickbarParse {
        envelope: payload[0],
        declared: u32::try_from(read_buffer.len().checked_add(3)?).ok()?,
        read_size: read_buffer.len(),
        fragment_size: 0,
        final_cursor,
        buttons,
        direct_opcode_stream: true,
    })
}

pub(super) fn parse_quickbar_read_buffer(
    read_buffer: &[u8],
    mut cursor: usize,
) -> Option<(Vec<QuickbarButton>, usize)> {
    let model_types = quickbar_base_item_model_types()?;
    let mut buttons = Vec::with_capacity(LEGACY_QUICKBAR_BUTTON_COUNT);
    let memo_width = read_buffer.len().checked_add(1)?;
    let mut memo =
        vec![QUICKBAR_UNKNOWN_SCORE; (LEGACY_QUICKBAR_BUTTON_COUNT + 1).checked_mul(memo_width)?];
    for slot in 0..LEGACY_QUICKBAR_BUTTON_COUNT {
        if cursor >= read_buffer.len() {
            buttons.extend(
                (slot..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| QuickbarButton {
                    kind: QuickbarButtonKind::Unsupported,
                }),
            );
            break;
        }

        let ty = read_buffer[cursor];
        if ty == 1 {
            buttons.push(QuickbarButton {
                kind: QuickbarButtonKind::ItemCandidate,
            });
            cursor =
                choose_legacy_quickbar_item_end(read_buffer, slot, cursor, model_types, &mut memo)
                    .filter(|next_cursor| *next_cursor > cursor)
                    .unwrap_or_else(|| cursor.saturating_add(1));
            continue;
        }

        let parsed = parse_legacy_quickbar_non_item(read_buffer, cursor).or_else(|| {
            let resync_cursor = find_legacy_quickbar_resync(read_buffer, slot, cursor)?;
            parse_legacy_quickbar_non_item(read_buffer, resync_cursor)
        });
        let (button, next_cursor) = parsed.unwrap_or((
            QuickbarButton {
                kind: QuickbarButtonKind::Unsupported,
            },
            cursor.saturating_add(1),
        ));
        if next_cursor <= cursor || next_cursor > read_buffer.len() {
            return None;
        }
        buttons.push(button);
        cursor = next_cursor;
    }

    if buttons.len() != LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }
    Some((buttons, cursor.min(read_buffer.len())))
}

pub(super) fn parse_quickbar_read_buffer_with_fragments(
    read_buffer: &[u8],
    fragments: &[u8],
    cursor: usize,
) -> Option<(Vec<QuickbarButton>, usize)> {
    if fragments.is_empty() {
        return None;
    }
    let model_types = quickbar_base_item_model_types()?;
    let mut reader = QuickbarPacketReader {
        read_buffer,
        fragments,
        cursor,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };
    reader.final_fragment_bits = reader.read_bits(3)? as u8;

    let mut buttons = Vec::with_capacity(LEGACY_QUICKBAR_BUTTON_COUNT);
    let memo_width = read_buffer.len().checked_add(1)?;
    let mut memo =
        vec![QUICKBAR_UNKNOWN_SCORE; (LEGACY_QUICKBAR_BUTTON_COUNT + 1).checked_mul(memo_width)?];
    let mut opaque_item_slots_blanked = false;
    let mut allow_fragment_tail_slack = false;
    for slot in 0..LEGACY_QUICKBAR_BUTTON_COUNT {
        if reader.cursor >= read_buffer.len() {
            buttons.extend(
                (slot..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| QuickbarButton {
                    kind: QuickbarButtonKind::Unsupported,
                }),
            );
            opaque_item_slots_blanked = true;
            break;
        }

        let before_button = reader.clone();
        let button_start = reader.cursor;
        let ty = reader.read_byte()?;
        if ty == 1 {
            let payload_start = reader.cursor;
            if let Some((primary, secondary)) =
                parse_legacy_quickbar_item_payload(&mut reader, model_types)
            {
                // EE/Diamond SetAllButtons streams are 36 inline button
                // records.  A type-1 item with both item-presence BOOLs false
                // owns no item read-buffer bytes. If the decompile-owned item
                // boundary scorer can still prove a later next-button boundary,
                // then the fragment cursor is not aligned for this legacy body.
                // Blank this unowned item instead of continuing out of phase or
                // forwarding unknown bytes.
                if !primary.present && !secondary.present && reader.cursor == payload_start {
                    // Some HG SetAllButtons captures carry byte-owned compact
                    // item objects at this cursor while the shared fragment
                    // stream is already out of phase for the two Diamond/EE
                    // presence BOOLs.  The decompiled EE/Diamond item reader
                    // still owns the BOOL-gated shape, so do not reinterpret a
                    // false presence bit as "present".  Instead, treat this as
                    // the same legacy compact source variant handled below:
                    // only materialize an item when the focused byte-side item
                    // parser proves a complete object model and the bounded
                    // 36-slot scorer proves the next quickbar boundary.
                    if let Some((next_cursor, primary, secondary)) =
                        choose_legacy_quickbar_compact_item_end(
                            read_buffer,
                            slot,
                            payload_start,
                            model_types,
                            &mut memo,
                        )
                    {
                        tracing::info!(
                            slot,
                            button_start,
                            payload_start,
                            next_cursor,
                            fragment_cursor = reader.fragment_cursor,
                            fragment_bit = reader.fragment_bit,
                            "server GuiQuickbar_SetAllButtons translated compact byte-owned item tail after absent presence bits"
                        );
                        reader = before_button;
                        reader.cursor = next_cursor;
                        allow_fragment_tail_slack = true;
                        buttons.push(QuickbarButton {
                            kind: QuickbarButtonKind::Item {
                                primary,
                                secondary,
                                source: QuickbarItemSource::CompactByteOwnedWithSourceType,
                                recovered_type_tag: false,
                            },
                        });
                        continue;
                    }
                    let next_cursor = choose_legacy_quickbar_item_end(
                        read_buffer,
                        slot,
                        button_start,
                        model_types,
                        &mut memo,
                    )
                    .filter(|next_cursor| *next_cursor > payload_start);
                    if let Some(next_cursor) = next_cursor {
                        tracing::info!(
                            slot,
                            button_start,
                            payload_start,
                            next_cursor,
                            fragment_cursor = reader.fragment_cursor,
                            fragment_bit = reader.fragment_bit,
                            "server GuiQuickbar_SetAllButtons blanked item candidate after absent presence bits"
                        );
                        reader = before_button;
                        reader.cursor = next_cursor;
                        opaque_item_slots_blanked = true;
                        buttons.push(QuickbarButton {
                            kind: QuickbarButtonKind::ItemCandidate,
                        });
                        continue;
                    }
                }
                buttons.push(QuickbarButton {
                    kind: QuickbarButtonKind::Item {
                        primary,
                        secondary,
                        source: QuickbarItemSource::ExplicitTypeAndFragmentBits,
                        recovered_type_tag: false,
                    },
                });
            } else {
                // Decompile-backed quickbar discipline:
                // `P 1E 01` contains exactly 36 button records. If a type-1
                // item object cannot be translated because its legacy
                // item/active-property body is not yet owned by the Rust
                // parser, do not forward those bytes and do not blank the whole
                // bar. Use the bounded legacy item-end scorer from the mature
                // bridge to find the next plausible button boundary, blank this
                // item, and continue preserving later spell/general buttons.
                // The shared fragment tail may contain only the skipped item
                // BOOLs, so final fragment exhaustion is required only when no
                // opaque item slot was blanked.
                if let Some((next_cursor, primary, secondary)) =
                    choose_legacy_quickbar_compact_item_end(
                        read_buffer,
                        slot,
                        payload_start,
                        model_types,
                        &mut memo,
                    )
                {
                    tracing::info!(
                        slot,
                        button_start,
                        payload_start,
                        next_cursor,
                        fragment_cursor = reader.fragment_cursor,
                        fragment_bit = reader.fragment_bit,
                        "server GuiQuickbar_SetAllButtons translated compact byte-owned item tail after fragment-bit parse failure"
                    );
                    reader = before_button;
                    reader.cursor = next_cursor;
                    allow_fragment_tail_slack = true;
                    buttons.push(QuickbarButton {
                        kind: QuickbarButtonKind::Item {
                            primary,
                            secondary,
                            source: QuickbarItemSource::CompactByteOwnedWithSourceType,
                            recovered_type_tag: false,
                        },
                    });
                    continue;
                }
                let next_cursor = choose_legacy_quickbar_item_end(
                    read_buffer,
                    slot,
                    button_start,
                    model_types,
                    &mut memo,
                )
                .filter(|next_cursor| *next_cursor > button_start);
                let Some(next_cursor) = next_cursor else {
                    if quickbar_can_blank_remaining_after_source_parse_failure(&buttons, slot) {
                        tracing::info!(
                            slot,
                            button_start,
                            fragment_cursor = reader.fragment_cursor,
                            fragment_bit = reader.fragment_bit,
                            "server GuiQuickbar_SetAllButtons blanked remaining slots after unowned item parse failure"
                        );
                        buttons.push(QuickbarButton {
                            kind: QuickbarButtonKind::ItemCandidate,
                        });
                        buttons.extend((slot + 1..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| {
                            QuickbarButton {
                                kind: QuickbarButtonKind::Unsupported,
                            }
                        }));
                        opaque_item_slots_blanked = true;
                        break;
                    }
                    return None;
                };
                tracing::info!(
                    slot,
                    button_start,
                    next_cursor,
                    fragment_cursor = reader.fragment_cursor,
                    fragment_bit = reader.fragment_bit,
                    "server GuiQuickbar_SetAllButtons blanked item candidate after unowned item parse failure"
                );
                reader = before_button;
                reader.cursor = next_cursor;
                opaque_item_slots_blanked = true;
                buttons.push(QuickbarButton {
                    kind: QuickbarButtonKind::ItemCandidate,
                });
            }
            continue;
        }
        let kind = if let Some(kind) = parse_legacy_quickbar_non_item_from_reader(&mut reader, ty) {
            kind
        } else {
            // Compatibility transform for an HG/Diamond compact item source
            // shape observed in captured `P 1E 01` SetAllButtons streams:
            // after earlier empty slots, the item object body can begin at the
            // next slot boundary without Diamond's explicit type-1 byte.  EE's
            // decompiled writer always emits the slot type byte before the
            // case-1 item BOOL/object/appearance/property branches, so the
            // bridge must not forward this source shape raw.  Instead, only
            // accept it when the existing compact item parser proves a bounded
            // item model and the remaining 36-slot quickbar scorer proves the
            // next button boundary; the EE writer then restores the explicit
            // type tag from the typed model.
            if let Some((next_cursor, primary, secondary)) = choose_legacy_quickbar_compact_item_end(
                read_buffer,
                slot,
                button_start,
                model_types,
                &mut memo,
            ) {
                tracing::info!(
                    slot,
                    button_start,
                    next_cursor,
                    fragment_cursor = reader.fragment_cursor,
                    fragment_bit = reader.fragment_bit,
                    "server GuiQuickbar_SetAllButtons recovered compact item body with missing source type tag"
                );
                reader = before_button;
                reader.cursor = next_cursor;
                allow_fragment_tail_slack = true;
                buttons.push(QuickbarButton {
                    kind: QuickbarButtonKind::Item {
                        primary,
                        secondary,
                        source: QuickbarItemSource::RecoveredMissingType,
                        recovered_type_tag: true,
                    },
                });
                continue;
            }
            let Some(resync_cursor) = find_legacy_quickbar_resync(read_buffer, slot, button_start)
            else {
                if quickbar_can_blank_remaining_after_source_parse_failure(&buttons, slot) {
                    reader = before_button;
                    buttons.push(QuickbarButton {
                        kind: QuickbarButtonKind::Unsupported,
                    });
                    buttons.extend((slot + 1..LEGACY_QUICKBAR_BUTTON_COUNT).map(|_| {
                        QuickbarButton {
                            kind: QuickbarButtonKind::Unsupported,
                        }
                    }));
                    opaque_item_slots_blanked = true;
                    break;
                }
                return None;
            };
            reader = before_button;
            reader.cursor = resync_cursor;
            let resynced_type = reader.read_byte()?;
            parse_legacy_quickbar_non_item_from_reader(&mut reader, resynced_type)?
        };
        if slot + 1 == LEGACY_QUICKBAR_BUTTON_COUNT
            && ty == 18
            && reader.cursor.checked_add(CNW_LENGTH_BYTES)? == read_buffer.len()
        {
            // HG command-line quickbar records have been observed with a final
            // four-byte read-window suffix after Diamond's decompile-owned two
            // CExoString payload. General command buttons are not emitted raw
            // yet, so consume this exact final-slot suffix only to prove the
            // 36-slot boundary, then let the writer emit a blank EE slot.
            reader.skip_bytes(CNW_LENGTH_BYTES)?;
        }
        buttons.push(QuickbarButton { kind });
    }

    if buttons.len() != LEGACY_QUICKBAR_BUTTON_COUNT {
        return None;
    }
    if !opaque_item_slots_blanked && reader.cursor != read_buffer.len() {
        if allow_fragment_tail_slack
            && legacy_quickbar_trailing_command_tail_is_discardable(read_buffer, reader.cursor)
        {
            tracing::info!(
                cursor = reader.cursor,
                read_len = read_buffer.len(),
                trailing = read_buffer.len().saturating_sub(reader.cursor),
                "server GuiQuickbar_SetAllButtons discarded extra legacy command tail after compact item recovery"
            );
            reader.cursor = read_buffer.len();
        } else {
            return None;
        }
    }
    let consumed_fragment_bits = reader
        .fragment_cursor
        .checked_mul(8)?
        .checked_add(usize::from(reader.fragment_bit))?;
    let consumed_fragment_bytes = reader.fragment_cursor + usize::from(reader.fragment_bit != 0);
    // Compact byte-owned HG item records use a verified compatibility parser
    // for active-property BOOL fields that Diamond/EE normally read from the
    // fragment stream. The writer discards the legacy fragment tail and emits
    // fresh EE bits from the typed model, so unused source bits are allowed
    // here; reading beyond the tail is still invalid.
    if allow_fragment_tail_slack && consumed_fragment_bytes > fragments.len() {
        return None;
    }
    if !opaque_item_slots_blanked
        && !allow_fragment_tail_slack
        && (consumed_fragment_bytes != fragments.len()
            || reader.final_fragment_bits != u8::try_from(consumed_fragment_bits % 8).ok()?)
    {
        return None;
    }
    Some((buttons, reader.cursor.min(read_buffer.len())))
}

fn quickbar_can_blank_remaining_after_source_parse_failure(
    buttons: &[QuickbarButton],
    slot: usize,
) -> bool {
    // EE/Diamond both define `GuiQuickbar_SetAllButtons` as exactly 36 slot
    // records. The C++ bridge's decompile-backed path used this as a semantic
    // boundary: once at least one earlier slot has been parsed or the failure
    // occurs after slot zero, unowned later source bytes may be consumed and
    // emitted as empty EE slots, but they must never be forwarded raw.
    slot > 0
        || buttons.iter().any(|button| {
            matches!(
                button.kind,
                QuickbarButtonKind::Spell { .. }
                    | QuickbarButtonKind::General { .. }
                    | QuickbarButtonKind::Item { .. }
                    | QuickbarButtonKind::ItemCandidate
            )
        })
}

fn legacy_quickbar_trailing_command_tail_is_discardable(read_buffer: &[u8], cursor: usize) -> bool {
    // HG captures can end the recovered compact-item read window with one
    // Diamond command-line quickbar record that was not part of the 36 slots
    // selected by the typed scorer. The decompile-owned command shape is type
    // 18 followed by two CExoString fields; existing code already treats the
    // final four-byte HG suffix after that shape as a boundary-only empty
    // string-length artifact. Do not discard an arbitrary DWORD here: Diamond
    // `sub_469FD0` and EE `sub_14079DB00` own only the two command CExoStrings.
    // General command buttons are never emitted raw by this translator, so this
    // exact tail may be consumed only to prove the SetAllButtons boundary.
    let mut probe = QuickbarPacketReader {
        read_buffer,
        fragments: &[],
        cursor,
        fragment_cursor: 0,
        fragment_bit: 0,
        final_fragment_bits: 0,
    };
    if probe.read_byte() != Some(18) {
        return false;
    }
    if probe.skip_string().is_none() || probe.skip_string().is_none() {
        return false;
    }
    if probe.cursor == read_buffer.len() {
        return true;
    }
    probe
        .cursor
        .checked_add(CNW_LENGTH_BYTES)
        .is_some_and(|end| end == read_buffer.len())
        && read_u32_le(read_buffer, probe.cursor) == Some(0)
}

fn parse_legacy_quickbar_non_item_from_reader(
    reader: &mut QuickbarPacketReader<'_>,
    ty: u8,
) -> Option<QuickbarButtonKind> {
    if !is_legacy_quickbar_plausible_type(ty) || ty == 1 {
        return None;
    }

    let start = reader.cursor.checked_sub(1)?;
    if legacy_quickbar_type_has_no_payload(ty) {
        return Some(QuickbarButtonKind::General { bytes: vec![ty] });
    }

    if ty == 2 {
        let class_byte = reader.read_byte()?;
        let spell_id = reader.read_dword()?;
        let legacy_metamagic = reader.read_byte()?;
        let legacy_level = reader.read_byte()?;
        if spell_id > 10_000 {
            return Some(QuickbarButtonKind::Unsupported);
        }
        return Some(QuickbarButtonKind::Spell {
            class_byte,
            spell_id,
            legacy_metamagic,
            legacy_level,
        });
    }

    if legacy_quickbar_type_has_int_payload(ty) {
        let value = reader.read_dword()?;
        if !legacy_quickbar_int_payload_is_valid_for_ee(ty, value) {
            return Some(QuickbarButtonKind::Unsupported);
        }
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if ty == 44 {
        reader.skip_bytes(CNW_LENGTH_BYTES + 1)?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if (11..=17).contains(&ty) {
        reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        reader.skip_string()?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if ty == 18 {
        reader.skip_string()?;
        reader.skip_string()?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    if ty == 29 || ty == 30 {
        reader.skip_bytes(C_RESREF_TEXT_BYTES)?;
        return Some(QuickbarButtonKind::General {
            bytes: reader.read_buffer.get(start..reader.cursor)?.to_vec(),
        });
    }

    None
}

pub(super) fn parse_legacy_quickbar_non_item(
    read_buffer: &[u8],
    cursor: usize,
) -> Option<(QuickbarButton, usize)> {
    let ty = *read_buffer.get(cursor)?;
    if !is_legacy_quickbar_plausible_type(ty) || ty == 1 {
        return None;
    }

    if legacy_quickbar_type_has_no_payload(ty) {
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes: vec![ty] },
            },
            cursor + 1,
        ));
    }

    let payload_cursor = cursor.checked_add(1)?;
    if ty == 2 {
        let class_byte = *read_buffer.get(payload_cursor)?;
        let spell_id = read_u32_le(read_buffer, payload_cursor + 1)?;
        let legacy_metamagic = *read_buffer.get(payload_cursor + 5)?;
        let legacy_level = *read_buffer.get(payload_cursor + 6)?;
        if spell_id > 10_000 {
            return Some((
                QuickbarButton {
                    kind: QuickbarButtonKind::Unsupported,
                },
                payload_cursor + 7,
            ));
        }
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::Spell {
                    class_byte,
                    spell_id,
                    legacy_metamagic,
                    legacy_level,
                },
            },
            payload_cursor + 7,
        ));
    }

    if legacy_quickbar_type_has_int_payload(ty) {
        let next_cursor = payload_cursor.checked_add(4)?;
        let value = read_u32_le(read_buffer, payload_cursor)?;
        if !legacy_quickbar_int_payload_is_valid_for_ee(ty, value) {
            return Some((
                QuickbarButton {
                    kind: QuickbarButtonKind::Unsupported,
                },
                next_cursor,
            ));
        }
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if ty == 44 {
        let next_cursor = payload_cursor.checked_add(5)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if (11..=17).contains(&ty) {
        let after_resref = payload_cursor.checked_add(C_RESREF_TEXT_BYTES)?;
        let next_cursor = advance_legacy_quickbar_string(read_buffer, after_resref)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if ty == 18 {
        let after_first = advance_legacy_quickbar_string(read_buffer, payload_cursor)?;
        let next_cursor = advance_legacy_quickbar_string(read_buffer, after_first)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    if ty == 29 || ty == 30 {
        let next_cursor = payload_cursor.checked_add(C_RESREF_TEXT_BYTES)?;
        let bytes = read_buffer.get(cursor..next_cursor)?.to_vec();
        return Some((
            QuickbarButton {
                kind: QuickbarButtonKind::General { bytes },
            },
            next_cursor,
        ));
    }

    None
}

fn advance_legacy_quickbar_string(read_buffer: &[u8], cursor: usize) -> Option<usize> {
    let length = usize::try_from(read_u32_le(read_buffer, cursor)?).ok()?;
    if length > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
        return None;
    }
    cursor.checked_add(CNW_LENGTH_BYTES)?.checked_add(length)
}

pub(super) fn is_legacy_quickbar_plausible_type(ty: u8) -> bool {
    ty <= 48
}

pub(super) fn legacy_quickbar_type_has_no_payload(ty: u8) -> bool {
    matches!(
        ty,
        // Diamond 1.69's `sub_469FD0` quickbar receiver maps type 9 to the
        // default/no-extra-read path in its jump table. EE's server sender uses
        // type 9 as an item-bearing shape, so the writer must not copy this
        // byte through unchanged; it consumes the legacy one-byte record and
        // emits a known-valid blank EE slot.
        0 | 5 | 6 | 7 | 9 | 19 | 20 | 21 | 22 | 23 | 24 | 25 | 26 | 35 | 36 | 38 | 40 | 41
    )
}

pub(super) fn legacy_quickbar_type_has_int_payload(ty: u8) -> bool {
    matches!(
        ty,
        3 | 4 | 8 | 10 | 27 | 28 | 31 | 32 | 33 | 34 | 37 | 42 | 43 | 45 | 46 | 47 | 48
    )
}

pub(super) fn legacy_quickbar_int_payload_is_valid_for_ee(ty: u8, value: u32) -> bool {
    match ty {
        // EE's quickbar case 8 reads `ReadINT(32)`, then calls `sub_14086B160`.
        // That path reaches `sub_140866C90`, which stores the value and indexes
        // `off_141297500[value]` plus `dword_140E46CF0[value]` directly. The
        // 8193.37.17 decompile shows 23 animation/icon entries (indices 0..22).
        // Therefore this is still a strict semantic translation: in-range
        // values are byte-identical and preserved, while out-of-range values
        // are consumed and emitted as an empty slot instead of raw passthrough.
        8 => value < EE_QUICKBAR_ANIMATION_ICON_COUNT,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_tail_with_optional_suffix(suffix: &[u8]) -> Vec<u8> {
        let mut read_buffer = vec![18];
        read_buffer.extend_from_slice(&0u32.to_le_bytes());
        read_buffer.extend_from_slice(&0u32.to_le_bytes());
        read_buffer.extend_from_slice(suffix);
        read_buffer
    }

    #[test]
    fn quickbar_command_tail_allows_exact_command_shape_without_suffix() {
        let read_buffer = command_tail_with_optional_suffix(&[]);

        assert!(legacy_quickbar_trailing_command_tail_is_discardable(
            &read_buffer,
            0
        ));
    }

    #[test]
    fn quickbar_command_tail_allows_only_empty_suffix_length() {
        let read_buffer = command_tail_with_optional_suffix(&0u32.to_le_bytes());

        assert!(legacy_quickbar_trailing_command_tail_is_discardable(
            &read_buffer,
            0
        ));
    }

    #[test]
    fn quickbar_command_tail_rejects_nonzero_suffix_dword() {
        let read_buffer = command_tail_with_optional_suffix(&1u32.to_le_bytes());

        assert!(
            !legacy_quickbar_trailing_command_tail_is_discardable(&read_buffer, 0),
            "command-tail compatibility must not discard arbitrary read-buffer DWORDs"
        );
    }
}
