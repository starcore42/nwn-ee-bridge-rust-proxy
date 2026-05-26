//! Fixture-free live-object update regression anchors.

fn live_gui_character_sheet_payload(mask: u32, body: &[u8], owned_bits: Vec<bool>) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'G', b'S']);
    live.extend_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    live.extend_from_slice(&mask.to_le_bytes());
    live.extend_from_slice(body);

    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);

    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS];
    fragment_bits.extend(owned_bits);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

fn live_gui_read_buffer_payload(live: &[u8]) -> Vec<u8> {
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

fn push_msb_bits(bits: &mut Vec<bool>, value: u32, width: usize) {
    for shift in (0..width).rev() {
        bits.push(((value >> shift) & 1) != 0);
    }
}

fn creature_status_effect_4008_payload(rows: &[(u16, Option<&[u8]>)]) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x05, 0x55, 0x00, 0x00, 0x80]);
    live.extend_from_slice(&0x0000_4008u32.to_le_bytes());
    live.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    for (row, target_payload) in rows {
        live.push(b'A');
        live.extend_from_slice(&row.to_le_bytes());
        if let Some(payload) = target_payload {
            live.extend_from_slice(payload);
        }
        live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    }

    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS + 7],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

#[test]
fn live_gui_inventory_update_row_is_ten_read_buffer_bytes() {
    // Diamond `sub_4589A0` and EE `sub_1407B3F30` read inventory `G I/i U` as
    // inner opcode, OBJECTID/INT32, SHORT, BYTE. Unlike repository `G R/r U`,
    // it does not own two trailing DWORDs or any CNW fragment bits.
    let mut live = vec![b'G', b'I', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x3344u16.to_le_bytes());
    live.push(0x55);

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory GUI update row should exact-claim at ten bytes");

    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 10);
}

#[test]
fn live_gui_inventory_update_splits_before_following_gui_row() {
    let mut live = vec![b'G', b'i', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x3344u16.to_le_bytes());
    live.push(0x55);
    live.extend_from_slice(&[b'G', b'Q', 0]);

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory GUI update should hand off before the following GQ row");

    assert_eq!(claim.live_gui_read_buffer_records, 2);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 13);
}

#[test]
fn live_gui_inventory_update_rejects_repository_width_tail() {
    let mut live = vec![b'G', b'I', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x3344u16.to_le_bytes());
    live.push(0x55);
    live.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD, 0xEE]);

    let payload = live_gui_read_buffer_payload(&live);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "inventory G I U must not swallow five repository-width tail bytes"
    );
}

#[test]
fn live_gui_repository_update_remains_fifteen_read_buffer_bytes() {
    let mut live = vec![b'G', b'R', b'U'];
    live.extend_from_slice(&0x8001_2345u32.to_le_bytes());
    live.extend_from_slice(&0x1122_3344u32.to_le_bytes());
    live.extend_from_slice(&0x5566_7788u32.to_le_bytes());

    let payload = live_gui_read_buffer_payload(&live);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("repository GUI update row should keep the wider exact shape");

    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 0);
    assert_eq!(claim.live_bytes_length, 15);
}

#[test]
fn live_gui_character_sheet_effect_icons_word_ids_do_not_split_on_legacy_prefix() {
    // EE build 8193.37 widened character-sheet effect-icon counts and ids to
    // WORDs. The leading zero removed-count byte is also a valid legacy prefix,
    // so exact ownership must pick the full word-id branch before final cursor
    // validation.
    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0x1234u16.to_le_bytes());

    let payload = live_gui_character_sheet_payload(0x0000_0100, &body, vec![true]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("word-id character-sheet effect icons should exact-claim");
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 1);
    assert_eq!(claim.live_bytes_length, 16);
}

#[test]
fn live_gui_character_sheet_effect_icons_word_ids_require_changed_row_bool() {
    let mut body = Vec::new();
    body.extend_from_slice(&0u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&0x1234u16.to_le_bytes());

    let payload = live_gui_character_sheet_payload(0x0000_0100, &body, Vec::new());

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "the changed effect-icon row owns one CNW BOOL after the WORD id"
    );
}

#[test]
fn live_gui_character_sheet_combat_build_8193_35_owns_five_bit_actions() {
    // EE build 8193.35 widened the second combat-info list action field from
    // four bits to five. The byte window is identical to the legacy shape, so
    // the candidate selector has to use final fragment ownership, not first
    // parse success.
    let mut body = Vec::new();
    let mut bits = Vec::new();

    push_msb_bits(&mut bits, 0, 3);
    body.extend_from_slice(&[0x11, 0x22, 0x33]);
    push_msb_bits(&mut bits, 0, 7);
    push_msb_bits(&mut bits, 0, 5);
    push_msb_bits(&mut bits, 0, 5);
    push_msb_bits(&mut bits, 0, 5);
    for row in 0..3 {
        push_msb_bits(&mut bits, 0, 5);
        push_msb_bits(&mut bits, 0, 5);
        body.push(0x40 + row);
    }
    push_msb_bits(&mut bits, 0, 4);
    push_msb_bits(&mut bits, 0, 3);
    bits.push(false);
    body.push(0);
    body.push(1);
    body.push(0x77);
    push_msb_bits(&mut bits, 0b1_0001, 5);
    push_msb_bits(&mut bits, 0b101, 3);
    bits.extend_from_slice(&[false, false, false]);

    let payload = live_gui_character_sheet_payload(0x0000_0040, &body, bits.clone());

    let claim = super::claim_payload_if_verified(&payload)
        .expect("five-bit character-sheet combat action should exact-claim");
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_fragment_bits, bits.len() as u32);
}

#[test]
fn creature_status_effect_single_target_payload_is_exact_ee_shape() {
    let payload =
        creature_status_effect_4008_payload(&[(0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66]))]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("single target-payload status-effect row should be exact-owned");
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn creature_status_effect_three_byte_target_payload_is_not_exact_ee_shape() {
    let payload = creature_status_effect_4008_payload(&[(0x1234, Some(&[0x44, 0x33, 0x22]))]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "EE sub_1407B1F00 and Diamond sub_44ED20 own DWORD target id plus one BYTE, not a three-byte target payload"
    );
}

#[test]
fn creature_status_effect_multi_target_payload_stays_unclaimed_without_2da() {
    let payload = creature_status_effect_4008_payload(&[
        (0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66])),
        (0x1235, Some(&[0x55, 0x44, 0x33, 0x80, 0x77])),
    ]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da row-type proof, multi-row target payload boundaries are ambiguous"
    );
}

#[test]
fn creature_status_effect_mixed_target_payload_rows_stay_unclaimed_without_2da() {
    let payload = creature_status_effect_4008_payload(&[
        (0x00F3, None),
        (0x1234, Some(&[0x44, 0x33, 0x22, 0x80, 0x66])),
    ]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da row-type proof, mixed target/no-target rows cannot be exact-owned"
    );
}

fn c40f_creature_update_with_adjacent_fragment_span() -> (Vec<u8>, usize, Vec<bool>, Vec<u8>) {
    let mut live = vec![b'U', 0x05, 0x55, 0x00, 0x00, 0x80];
    live.extend_from_slice(&0x0000_C40Fu32.to_le_bytes());
    live.extend_from_slice(&[0; 6]); // 0x0001 position: WORD, WORD, WORD + 2 bits.
    live.push(0); // 0x0002 scalar orientation: one BYTE + four bits.
    live.extend_from_slice(&0u32.to_le_bytes()); // 0x0004 action scalar.
    live.extend_from_slice(&0u16.to_le_bytes()); // 0x0004 action code.
    live.extend_from_slice(&0u16.to_le_bytes());
    live.extend_from_slice(&[0; 8]); // 0x0400 four WORD scalar/status values.
    let read_end = live.len();
    let span = super::bits::pack_msb_valid_bits(
        vec![false, false, false, true, false, true],
        super::CNW_FRAGMENT_HEADER_BITS,
    );
    live.extend_from_slice(&span);

    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS + 18];
    // Bit order at the decompiled C40F cursor:
    //   0x0001 owns two residual position bits,
    //   0x0002 owns scalar-orientation selector + one BYTE + four scalar bits,
    //   the optional orientation target guard can own one BOOL,
    //   0x4000 owns seven status BOOLs,
    //   0x8000 owns three self-visibility BOOLs.
    // The first scalar bit is true so starting one bit late makes the
    // orientation selector look like the vector branch and fail exact proof.
    fragment_bits[super::CNW_FRAGMENT_HEADER_BITS + 3] = true;

    (live, read_end, fragment_bits, span)
}

#[test]
fn creature_interleaved_fragment_span_requires_exact_bit_cursor() {
    let (mut live, read_end, mut fragment_bits, span) =
        c40f_creature_update_with_adjacent_fragment_span();
    let old_record_end = live.len();
    let mut record_end = old_record_end;

    let shifted_cursor = super::CNW_FRAGMENT_HEADER_BITS + 1;
    assert!(
        super::fragment_spans::promote_creature_update_interleaved_fragment_span_for_ee(
            &mut live,
            &mut fragment_bits,
            0,
            &mut record_end,
            shifted_cursor,
        )
        .is_none(),
        "the span promoter must not retry at a neighboring fragment cursor"
    );
    assert_eq!(record_end, old_record_end);
    assert_eq!(live.len(), old_record_end);
    assert_eq!(read_end + span.len(), old_record_end);
}

#[test]
fn creature_interleaved_fragment_span_promotes_from_exact_bit_cursor() {
    let (mut live, read_end, mut fragment_bits, span) =
        c40f_creature_update_with_adjacent_fragment_span();
    let old_record_end = live.len();
    let mut record_end = old_record_end;

    let promoted = super::fragment_spans::promote_creature_update_interleaved_fragment_span_for_ee(
        &mut live,
        &mut fragment_bits,
        0,
        &mut record_end,
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("the C40F span should promote from the exact inherited bit cursor");

    assert_eq!(promoted.read_end, read_end);
    assert_eq!(promoted.old_record_end, old_record_end);
    assert_eq!(promoted.bytes_promoted, span.len());
    assert_eq!(promoted.bits_promoted, 3);
    assert_eq!(record_end, read_end);
    assert_eq!(live.len(), read_end);
}
