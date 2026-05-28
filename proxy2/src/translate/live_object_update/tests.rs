//! Packet-level live-object update regression anchors.

fn ee_scalar_orientation_read_byte_from_legacy_facing(facing: u16) -> u8 {
    let scalar12 = super::writer::encode_ee_scalar_orientation_from_legacy_facing(facing);
    ((scalar12 >> 4) & 0xFF) as u8
}

fn ee_scalar_orientation_fragment_bits_from_legacy_facing(facing: u16) -> [bool; 5] {
    let scalar12 = super::writer::encode_ee_scalar_orientation_from_legacy_facing(facing);
    [
        false,
        ((scalar12 >> 3) & 1) != 0,
        ((scalar12 >> 2) & 1) != 0,
        ((scalar12 >> 1) & 1) != 0,
        (scalar12 & 1) != 0,
    ]
}

fn live_object_read_window(payload: &[u8]) -> &[u8] {
    let declared = usize::try_from(
        super::read_u32_le(payload, super::HIGH_LEVEL_HEADER_BYTES)
            .expect("fixture should have a declared CNW read length"),
    )
    .expect("declared CNW read length should fit usize");
    &payload[super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES..declared]
}

#[derive(Clone, Copy)]
enum TestCreatureAppearanceDialect {
    LegacyDiamond,
    EeBuild8193,
}

#[derive(Debug, PartialEq, Eq)]
struct TestFullCreatureAppearanceSemantics {
    object_id: u32,
    appearance_type: u16,
    body_selector: u8,
    equipment_count: u8,
    record_end: usize,
}

#[derive(Debug, PartialEq, Eq)]
struct TestFullCreatureAppearanceShape {
    object_id: u32,
    offset: usize,
    record_bytes: usize,
}

fn test_read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let bytes = bytes.get(offset..offset + 2)?;
    Some(u16::from_le_bytes([bytes[0], bytes[1]]))
}

fn test_cexo_string_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let length = usize::try_from(super::read_u32_le(bytes, offset)?).ok()?;
    if length > super::MAX_LIVE_OBJECT_NAME_BYTES {
        return None;
    }
    offset.checked_add(4)?.checked_add(length)
}

fn current_player_full_appearance_semantics(
    payload: &[u8],
    dialect: TestCreatureAppearanceDialect,
) -> Option<TestFullCreatureAppearanceSemantics> {
    let live = live_object_read_window(payload);
    for offset in live.windows(2).enumerate().filter_map(|(offset, pair)| {
        (pair == [b'P', super::CREATURE_OBJECT_TYPE]).then_some(offset)
    }) {
        let object_id = super::read_u32_le(live, offset + 2)?;
        let mask = test_read_u16_le(live, offset + 6)?;
        if object_id != 0xFFFF_FFFE || mask != 0xFFFF {
            continue;
        }

        let mut cursor = offset + 8;
        cursor = test_cexo_string_end(live, cursor)?;
        cursor = test_cexo_string_end(live, cursor)?;

        let appearance_type = test_read_u16_le(live, cursor)?;
        cursor = cursor.checked_add(2)?; // 0x0001
        cursor = cursor.checked_add(1)?; // 0x0002
        cursor = cursor.checked_add(1)?; // 0x0004
        cursor = cursor.checked_add(1)?; // 0x0080 low byte
        if matches!(dialect, TestCreatureAppearanceDialect::EeBuild8193) {
            cursor = cursor.checked_add(1)?; // EE build-0x23 high byte
        }
        cursor = cursor.checked_add(4)?; // 0x0800
        cursor = cursor.checked_add(4)?; // 0x1000
        cursor = cursor.checked_add(1)?; // 0x0008
        cursor = cursor.checked_add(1)?; // 0x0010
        cursor = cursor.checked_add(1)?; // 0x0020
        cursor = cursor.checked_add(1)?; // 0x0040

        let body_selector = *live.get(cursor)?;
        cursor = cursor.checked_add(1)?;
        if body_selector == 0 {
            // Existing body table unchanged.
        } else if body_selector < 0x0A {
            cursor = cursor.checked_add(usize::from(body_selector).checked_mul(2)?)?;
        } else {
            if body_selector > 0x13 {
                return None;
            }
            cursor = cursor.checked_add(match dialect {
                TestCreatureAppearanceDialect::LegacyDiamond => 0x13,
                TestCreatureAppearanceDialect::EeBuild8193 => 0x13 * 2,
            })?;
        }

        cursor = cursor.checked_add(2 + 4)?; // 0x2000 tail.
        if matches!(dialect, TestCreatureAppearanceDialect::EeBuild8193) {
            cursor = cursor.checked_add(1)?; // EE build-0x0E tail byte.
        }
        let equipment_count = *live.get(cursor)?;
        cursor = cursor.checked_add(1)?;
        return Some(TestFullCreatureAppearanceSemantics {
            object_id,
            appearance_type,
            body_selector,
            equipment_count,
            record_end: cursor,
        });
    }
    None
}

fn full_creature_appearance_shape(
    payload: &[u8],
    dialect: TestCreatureAppearanceDialect,
) -> Option<TestFullCreatureAppearanceShape> {
    let live = live_object_read_window(payload);
    live.windows(2)
        .enumerate()
        .filter_map(|(offset, pair)| {
            if pair != [b'P', super::CREATURE_OBJECT_TYPE] {
                return None;
            }
            let object_id = super::read_u32_le(live, offset.checked_add(2)?)?;
            let mask = test_read_u16_le(live, offset.checked_add(6)?)?;
            if object_id == 0 || mask != 0xFFFF {
                return None;
            }
            let record_end = match dialect {
                TestCreatureAppearanceDialect::LegacyDiamond => {
                    super::appearance::try_get_legacy_creature_appearance_record_end(
                        live,
                        offset,
                        live.len(),
                    )
                }
                TestCreatureAppearanceDialect::EeBuild8193 => {
                    super::appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
                        live,
                        offset,
                        live.len(),
                    )
                }
            }?;
            Some(TestFullCreatureAppearanceShape {
                object_id,
                offset,
                record_bytes: record_end.checked_sub(offset)?,
            })
        })
        .max_by_key(|shape| shape.record_bytes)
}

fn assert_no_full_creature_appearance_has_longer_legacy_shape(payload: &[u8]) {
    let live = live_object_read_window(payload);
    for offset in live.windows(2).enumerate().filter_map(|(offset, pair)| {
        (pair == [b'P', super::CREATURE_OBJECT_TYPE]).then_some(offset)
    }) {
        let Some(ee_end) =
            super::appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
                live,
                offset,
                live.len(),
            )
        else {
            continue;
        };
        let Some(legacy_end) = super::appearance::try_get_legacy_creature_appearance_record_end(
            live,
            offset,
            live.len(),
        ) else {
            continue;
        };
        assert!(
            legacy_end <= ee_end,
            "translated P/5 at offset {offset} still has a longer Diamond full-appearance shape than its EE byte shape: ee_end={ee_end} legacy_end={legacy_end}"
        );
    }
}

fn assert_rewrite_matches_expected_or_current_player_semantics(
    name: &str,
    legacy: &[u8],
    rewritten: &[u8],
    expected_ee: &[u8],
    context: &str,
) {
    if rewritten == expected_ee {
        return;
    }

    let legacy_shape =
        full_creature_appearance_shape(legacy, TestCreatureAppearanceDialect::LegacyDiamond);
    let expected_shape =
        full_creature_appearance_shape(expected_ee, TestCreatureAppearanceDialect::EeBuild8193);
    if let Some(legacy_shape) = legacy_shape {
        if expected_shape
            .as_ref()
            .map(|expected_shape| expected_shape.record_bytes < legacy_shape.record_bytes)
            .unwrap_or(true)
        {
            assert_ne!(
                rewritten, expected_ee,
                "{name} {context}: stale dumped EE bytes should not be reproduced"
            );
            assert!(
                rewritten.len() > expected_ee.len(),
                "{name} {context}: rewritten payload should retain bytes missing from the stale full-appearance dump"
            );
            assert!(
                super::claim_payload_if_verified(rewritten).is_some(),
                "{name} {context}: rewritten stale-class payload should exact-claim"
            );
            if let (Some(legacy_semantics), Some(rewritten_semantics)) = (
                current_player_full_appearance_semantics(
                    legacy,
                    TestCreatureAppearanceDialect::LegacyDiamond,
                ),
                current_player_full_appearance_semantics(
                    rewritten,
                    TestCreatureAppearanceDialect::EeBuild8193,
                ),
            ) {
                assert_eq!(
                    rewritten_semantics.appearance_type, legacy_semantics.appearance_type,
                    "{name} {context}: current-player Appearance_Type should round-trip"
                );
                assert_eq!(
                    rewritten_semantics.body_selector, legacy_semantics.body_selector,
                    "{name} {context}: current-player body selector should round-trip"
                );
                assert_eq!(
                    rewritten_semantics.equipment_count, legacy_semantics.equipment_count,
                    "{name} {context}: current-player equipment count should round-trip"
                );
            }
            assert_no_full_creature_appearance_has_longer_legacy_shape(rewritten);
            return;
        }
    }

    if let Some(legacy_semantics) = current_player_full_appearance_semantics(
        legacy,
        TestCreatureAppearanceDialect::LegacyDiamond,
    ) {
        if current_player_full_appearance_semantics(
            expected_ee,
            TestCreatureAppearanceDialect::EeBuild8193,
        )
        .is_none()
        {
            let rewritten_semantics = current_player_full_appearance_semantics(
                rewritten,
                TestCreatureAppearanceDialect::EeBuild8193,
            )
            .expect("rewritten current-player P/5 should preserve full EE appearance semantics");
            assert_ne!(
                rewritten, expected_ee,
                "{name} {context}: stale dumped EE bytes should not be reproduced"
            );
            assert_eq!(
                rewritten_semantics.appearance_type, legacy_semantics.appearance_type,
                "{name} {context}: current-player Appearance_Type should round-trip"
            );
            assert_eq!(
                rewritten_semantics.body_selector, legacy_semantics.body_selector,
                "{name} {context}: current-player body selector should round-trip"
            );
            assert_eq!(
                rewritten_semantics.equipment_count, legacy_semantics.equipment_count,
                "{name} {context}: current-player equipment count should round-trip"
            );
            assert_no_full_creature_appearance_has_longer_legacy_shape(rewritten);
            return;
        }
    }

    assert_eq!(rewritten, expected_ee, "{name} {context}");
}

fn rewrite_payload_to_exact_claim_for_test(
    payload: &mut Vec<u8>,
) -> super::LiveObjectUpdateClaimSummary {
    assert!(
        crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(payload, None),
        "live-object payload should rewrite through the bounded exact adapter"
    );
    super::claim_payload_if_verified(payload)
        .expect("rewritten live-object payload should exact-claim")
}

#[test]
fn legacy_facing_zero_preserves_shared_diamond_ee_packet_basis() {
    assert_eq!(
        super::writer::encode_ee_scalar_orientation_from_legacy_facing(0),
        0
    );
}

#[test]
fn legacy_facing_wraps_inside_ee_scalar_range() {
    assert!(super::writer::encode_ee_scalar_orientation_from_legacy_facing(u16::MAX) <= 0x0FFF);
}

#[test]
fn translated_door_update_record_is_exactly_claimed() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0x17, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(ee_scalar_orientation_read_byte_from_legacy_facing(0x2E00));
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());

    let mut fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained from the legacy stream.
    ];
    fragment_bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(
        0x2E00,
    ));
    fragment_bits.extend_from_slice(&[
        false, false, false, false, false, // five legacy door state bits.
        false, // EE-only neutral door state branch.
    ]);
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim = super::claim_payload_if_verified(&payload).expect("translated update claim");
    assert_eq!(claim.update_records, 1);
}

#[test]
fn legacy_name_bit_is_not_valid_in_ee_door_placeable_update_claims() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0x17, 0x00, 0x08, 0x00]);
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(ee_scalar_orientation_read_byte_from_legacy_facing(0x2E00));
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let mut fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained from the legacy stream.
    ];
    fragment_bits.extend_from_slice(&ee_scalar_orientation_fragment_bits_from_legacy_facing(
        0x2E00,
    ));
    fragment_bits.extend_from_slice(&[
        false, false, false, false, false, // five legacy door state bits.
        false, // EE-only neutral door state branch.
        false, // Legacy name bit payload that EE generic updates must not claim.
    ]);
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(super::claim_payload_if_verified(&payload).is_none());
}

#[test]
fn diamond_inline_name_door_update_drops_only_legacy_name_branch() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0x17, 0x00, 0x08, 0x00]);
    live.extend_from_slice(&[0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E]);
    live.push(0xD1);
    live.extend_from_slice(&1.0f32.to_le_bytes());
    live.extend_from_slice(&0x0016u16.to_le_bytes());
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained from the Diamond stream.
        false, false, false, false, false, // five Diamond door state bits.
        false, // Diamond-only name-presence branch.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("Diamond inline-name door update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert!(rewrite.bytes_removed >= 8);
    assert_eq!(rewrite.bits_inserted, 6);
    assert_eq!(rewrite.bits_removed, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated door mask");
    assert_eq!(translated_mask, 0x17);
    let claim = super::claim_payload_if_verified(&payload)
        .expect("Diamond inline-name update should become an exact EE door update");
    assert_eq!(
        payload[7 + super::LEGACY_UPDATE_HEADER_BYTES + super::LEGACY_UPDATE_POSITION_READ_BYTES],
        0xD1,
        "legacy 8-bit Diamond orientation byte must remain the high EE scalar byte"
    );
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    );
    let fragment_bits = fragment_bits.expect("rewritten door fragment bits");
    let orientation_bit_offset =
        super::CNW_FRAGMENT_HEADER_BITS + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    assert_eq!(
        &fragment_bits[orientation_bit_offset..orientation_bit_offset + 5],
        &[false, false, false, false, false],
        "Diamond has no orientation low-nibble fragment bits; EE scalar low bits are proxy-owned zero padding"
    );
}

#[test]
fn raw_legacy_all_bits_door_update_is_not_exactly_claimed() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16, 0x00,
    ]);
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let fragment_bits = vec![
        false, false, false, false, true, false, false, false, false, false,
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(super::claim_payload_if_verified(&payload).is_none());
}

#[test]
fn captured_len416_door_placeable_stream_with_nonrecord_leadin_stays_quarantined() {
    // Driver-only Starcore5 capture from 2026-05-13.
    //
    // This was originally quarantined as
    // `live-object-claimed-records-rejected-door-placeable-requires-translator`:
    // later bytes contain plausible door/placeable add/update records, but the
    // declared read window starts with `44 05 F7 20 50 72`, which is not a
    // decompile-backed live-object delete record because the following DWORD is
    // not an EE/Diamond object id.  The semantic translators must not scan past
    // that unowned lead-in and mutate later records; a future transport or
    // gameplay-stream owner must first prove where those leading bytes belong.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_legacy_door_placeable_len416_rejected_20260513.bin"
    )
    .to_vec();

    let update_pre = super::rewrite_update_records_payload_if_possible(&mut payload);
    let add = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let update_post = super::rewrite_update_records_payload_if_possible(&mut payload);
    let add_name_bits = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let add_after_name =
        crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
            &mut payload,
            None,
        );
    let update_after_name = super::rewrite_update_records_payload_if_possible(&mut payload);
    assert!(update_pre.is_none());
    assert!(add.is_none());
    assert!(update_post.is_none());
    assert!(add_name_bits.is_none());
    assert!(add_after_name.is_none());
    assert!(update_after_name.is_none());
    assert!(
        super::payload_contains_door_or_placeable_add_update_record(&payload),
        "fixture should still document the quarantined door/placeable-looking records"
    );
    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "non-record lead-in must keep this payload quarantined until a stream owner proves it"
    );
}

#[test]
fn legacy_all_bits_door_update_rewrites_to_exact_ee_claim() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x0A, 0xD1, 0x34, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0x8E, 0x12, 0xD4, 0x10, 0xEE, 0x0E, 0x00, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16, 0x00,
    ]);
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Door");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained.
        false, false, false, false, false, // five legacy door state bits.
        false, // Diamond name-presence BOOL removed with the legacy name branch.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy door update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated door mask");
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_ORIENTATION_MASK,
        super::LEGACY_UPDATE_ORIENTATION_MASK,
        "door orientation must survive mask translation so EE reads scalar facing"
    );
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_NAME_MASK,
        0,
        "Diamond's legacy 0x0008_0000 name bit is not an EE generic update bit"
    );
    let claim = super::claim_payload_if_verified(&payload).expect("translated update claim");
    assert_eq!(claim.update_records, 1);
    assert_eq!(
        payload[7 + super::LEGACY_UPDATE_HEADER_BYTES + super::LEGACY_UPDATE_POSITION_READ_BYTES],
        ee_scalar_orientation_read_byte_from_legacy_facing(0x2E00),
        "rewritten door update must carry the high eight CNW WriteUnsigned yaw bits in the read buffer"
    );
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten door fragment bits");
    let orientation_bit_offset =
        super::CNW_FRAGMENT_HEADER_BITS + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    assert_eq!(
        &fragment_bits[orientation_bit_offset..orientation_bit_offset + 5],
        &ee_scalar_orientation_fragment_bits_from_legacy_facing(0x2E00),
        "rewritten door update must carry the low four CNW WriteUnsigned yaw bits in the fragment stream"
    );
}

#[test]
fn legacy_all_bits_placeable_update_preserves_orientation_mask_when_rewritten() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0xC8, 0x35, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0xC4, 0x22, 0x84, 0x03, 0xA9, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x49, 0x00,
    ]);
    live.extend_from_slice(&22u32.to_le_bytes());
    live.extend_from_slice(b"Portal to Loot Testing");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained.
        false, false, false, false, false, // five legacy placeable state bits.
        false, // Diamond name-presence BOOL removed with the legacy name branch.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy placeable update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated placeable mask");
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_ORIENTATION_MASK,
        super::LEGACY_UPDATE_ORIENTATION_MASK,
        "placeable orientation must survive mask translation so signs keep facing"
    );
    assert_eq!(
        translated_mask & super::LEGACY_UPDATE_NAME_MASK,
        0,
        "Diamond's legacy 0x0008_0000 name bit is not an EE generic update bit"
    );
    let claim = super::claim_payload_if_verified(&payload).expect("translated placeable claim");
    assert_eq!(claim.update_records, 1);
    // The first six bytes after the legacy all-bits mask are packed position.
    // In this captured portal record the legacy facing WORD starts after that
    // block and is 0x0000, so this fixture proves the placeable path emits the
    // EE scalar-orientation field without inventing yaw from position bytes.
    assert_eq!(
        payload[7 + super::LEGACY_UPDATE_HEADER_BYTES + super::LEGACY_UPDATE_POSITION_READ_BYTES],
        ee_scalar_orientation_read_byte_from_legacy_facing(0x0000),
        "rewritten placeable update must carry the high eight CNW WriteUnsigned yaw bits in the read buffer"
    );
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten placeable fragment bits");
    let orientation_bit_offset =
        super::CNW_FRAGMENT_HEADER_BITS + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS;
    assert_eq!(
        &fragment_bits[orientation_bit_offset..orientation_bit_offset + 5],
        &ee_scalar_orientation_fragment_bits_from_legacy_facing(0x0000),
        "rewritten placeable update must carry the low four CNW WriteUnsigned yaw bits in the fragment stream"
    );
}

#[test]
fn low_bits_placeable_update_drops_bounded_tail_before_ee_claim() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0x07, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[
        0xBC, 0x04, 0x65, 0x04, 0x11, 0x0F, // position
        0x00, // scalar orientation high byte
        0x00, 0x00, // appearance word
        0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, // scale/state
        0x00, 0x00, 0x00, 0x00, // legacy low-bit control/name prefix
        b'L', b'u', b't', b'e', b' ', b'(', 0x00, 0x00, 0x00, 0x00, b'd', b'i', b'n', b'g', b')',
    ]);

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, true, // position low bits retained.
        false, false, false, false, false, // scalar orientation selector + low bits.
        false, false, false, false, false, // five Diamond placeable state bits.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("low-bit placeable update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.bytes_removed, 19);
    assert_eq!(rewrite.bits_inserted, 1);
    let translated_mask = super::read_u32_le(&payload, 7 + 6).expect("translated placeable mask");
    assert_eq!(translated_mask, 0x37);
    assert!(
        !live_object_read_window(&payload)
            .windows(b"Lute".len())
            .any(|window| window == b"Lute"),
        "bounded legacy low-bit placeable tail must not be forwarded to EE"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("low-bit placeable update should become an exact EE update");
    assert_eq!(claim.update_records, 1);
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten placeable fragment bits");
    let ee_placeable_extra_state_bit = super::CNW_FRAGMENT_HEADER_BITS
        + super::LEGACY_UPDATE_POSITION_FRAGMENT_BITS
        + super::EE_UPDATE_ORIENTATION_SCALAR_FRAGMENT_BITS
        + super::LEGACY_UPDATE_STATE_FRAGMENT_BITS;
    assert!(
        !fragment_bits[ee_placeable_extra_state_bit],
        "EE placeable-specific state reader consumes a neutral trailing BOOL"
    );
}

#[test]
fn low_bits_placeable_update_drops_cexo_tail_before_ee_claim() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0x09, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[
        0xBD, 0x02, 0xD7, 0x05, 0x9F, 0x00, // position
        0x00, // scalar orientation high byte
        0x00, 0x00, // appearance word
        0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, // scale/state
    ]);
    live.extend_from_slice(&4u32.to_le_bytes());
    live.extend_from_slice(b"Lute");

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, false, // position low bits retained.
        false, false, false, false, false, // scalar orientation selector + low bits.
        false, false, false, false, false, // five Diamond placeable state bits.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("low-bit placeable CExoString tail rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.bytes_removed, 8);
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(super::read_u32_le(&payload, 7 + 6), Some(0x37));
    let claim = super::claim_payload_if_verified(&payload)
        .expect("CExoString-tail placeable update should become exact EE");
    assert_eq!(claim.update_records, 1);
}

#[test]
fn low_bits_placeable_update_drops_word_zero_control_tail_before_ee_claim() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0x7C, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[
        0xDB, 0x33, 0xC0, 0x11, 0x41, 0x0F, // position
        0x5B, // scalar orientation high byte
        0x00, 0x00, // appearance word
        0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, // scale/state
        0x3B, 0x16, 0x00, 0x00, // legacy low-bit control WORD + zero WORD
    ]);

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, false, // position low bits retained.
        false, false, false, false, false, // scalar orientation selector + low bits.
        false, false, false, false, false, // five Diamond placeable state bits.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("low-bit placeable WORD/zero control-tail rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.bytes_removed, 4);
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(super::read_u32_le(&payload, 7 + 6), Some(0x37));
    let claim = super::claim_payload_if_verified(&payload)
        .expect("WORD/zero-tail placeable update should become exact EE");
    assert_eq!(claim.update_records, 1);
}

#[test]
fn low_bits_placeable_update_drops_absent_appearance_bit_when_prefix_is_exact() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0x19, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[
        0xF0, 0x03, 0xA7, 0x06, 0x0F, 0x00, // position
        0x00, // scalar orientation high byte
        0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, // scale/state, no appearance WORD.
    ]);

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header, rewritten by pack.
        false, false, // position low bits retained.
        false, false, false, false, false, // scalar orientation selector + low bits.
        false, false, false, false, false, // five Diamond placeable state bits.
    ];
    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("low-bit placeable absent-appearance rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.bytes_removed, 0);
    assert_eq!(rewrite.bits_inserted, 1);
    assert_eq!(
        super::read_u32_le(&payload, 7 + 6),
        Some(0x17),
        "EE/Diamond 0x20 consumes a WORD, so absent appearance bytes cannot keep that bit"
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("absent-appearance placeable update should become exact EE");
    assert_eq!(claim.update_records, 1);
}

#[test]
fn no_fragment_low_bits_placeable_update_inserts_neutral_source_bits() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0x7F, 0x00, 0x00, 0x80, 0xF7, 0x00, 0x00, 0x00]);
    live.extend_from_slice(&[
        0x89, 0x39, 0x02, 0x0D, 0x30, 0x10, // position
        0x9A, // scalar orientation high byte
        0x76, 0x00, // stale appearance word; absent from the scalar EE prefix
        0x00, 0x00, 0x80, 0x3F, 0x00, 0x00, // scale/state
        0x18, 0x16, 0x00, 0x00, // legacy low-bit control WORD + zero WORD
    ]);

    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("no-fragment low-bit placeable update rewrite");
    assert_eq!(rewrite.update_records_rewritten, 1);
    assert_eq!(rewrite.bytes_removed, 6);
    assert_eq!(rewrite.bits_inserted, 13);
    assert_eq!(
        super::read_u32_le(&payload, 7 + 6),
        Some(0x17),
        "absent appearance WORD is dropped with the Diamond-only low-bit suffix"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("no-fragment low-bit placeable update should become exact EE");
    assert_eq!(claim.update_records, 1);
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten placeable fragment bits");
    assert_eq!(fragment_bits.len(), super::CNW_FRAGMENT_HEADER_BITS + 13);
    assert!(
        fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..]
            .iter()
            .all(|bit| !*bit),
        "inserted no-fragment source bits are neutral"
    );
}

#[test]
fn no_fragment_empty_placeable_add_inserts_neutral_ee_guard_bits() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'A', 0x09, 0x85, 0x00, 0x00, 0x80]);
    live.extend_from_slice(&0u32.to_le_bytes());
    live.push(0x05);
    live.extend_from_slice(&0x000Eu16.to_le_bytes());
    live.extend_from_slice(&0u16.to_le_bytes());
    live.extend_from_slice(&super::visual_transform::EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);

    let mut payload = vec![b'P', 0x05, 0x01];
    let declared = (7 + live.len()) as u32;
    payload.extend_from_slice(&declared.to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("no-fragment empty placeable add guard rewrite");
    assert_eq!(rewrite.bits_inserted, 11);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("no-fragment empty placeable add should become exact EE");
    assert_eq!(claim.add_records, 1);
    let fragment_bits = super::bits::decode_msb_valid_bits(
        &payload[claim.declared..],
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("rewritten placeable add fragment bits");
    assert_eq!(fragment_bits.len(), super::CNW_FRAGMENT_HEADER_BITS + 11);
    assert!(
        fragment_bits[super::CNW_FRAGMENT_HEADER_BITS..]
            .iter()
            .all(|bit| !*bit),
        "inserted placeable add guard bits are neutral"
    );
}

#[test]
fn placeable_update_boundary_skips_anchored_tail_before_name() {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x09, 0xC8, 0x35, 0x00, 0x80, 0xF7, 0xFF, 0xFF, 0xFF]);
    live.extend_from_slice(&[
        0xC4, 0x22, 0x84, 0x03, 0xA9, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x80, 0x3F, 0x49, 0x00,
    ]);
    live.extend_from_slice(&22u32.to_le_bytes());
    live.extend_from_slice(b"Portal to Loot Testing");
    live.extend_from_slice(&[b'A', 0x09, 0xFB, 0x4C, 0x01, 0x80]);

    let record_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        &live,
        0,
        live.len(),
    );
    assert_eq!(record_end, 51);
}

#[test]
fn anchored_tail_rejects_non_string_four_byte_name_candidate() {
    let bytes = [
        0x38, 0x2E, 0x02, 0x00, 0x00, 0x80, 0x3F, 0x16, 0x00, 0xE5, 0x14, 0x00, 0x00,
    ];

    assert!(
        !super::reader::legacy_named_update_tail_following_payload_ready(&bytes, 0, bytes.len(),)
    );
}

#[test]
fn appearance_update_with_embedded_equipment_does_not_split_on_false_u09() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/player_appearance_false_u09.bin").to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("appearance rewrite");
    assert!(rewrite.bytes_inserted > 0);
    assert!(rewrite.bits_inserted > 0);
    let claim = super::claim_payload_if_verified(&payload).expect("appearance claim");
    assert_eq!(claim.records_examined, 3);
    assert_eq!(claim.creature_appearance_records, 1);
}

#[test]
fn appearance_update_slot0_visible_equipment_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/player_appearance_slot0_visible_equipment.bin"
    )
    .to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("slot0 appearance rewrite");
    assert!(rewrite.bytes_inserted > 0);
    assert!(rewrite.bits_inserted > 0);
    let claim = super::claim_payload_if_verified(&payload).expect("slot0 appearance claim");
    assert_eq!(claim.records_examined, 3);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_visual_transform_update_records, 0);
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn starcore_town_greeter_trader_stream_rewrites_to_exact_ee_live_object() {
    // HG Docks capture: the stream contains a creature add followed by a full
    // `P/5` creature appearance for Town Greeter and adjacent inventory/GUI
    // live-object records. Diamond writes this as a coherent live-object burst;
    // EE must only receive it after the add/update/appearance translators have
    // proven exact record boundaries and fragment-bit ownership.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/starcore_npc_town_greeter_trader_stream_claimed_but_ee_rejects.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Diamond burst must not be claimed as already-EE-shaped"
    );

    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("Town Greeter live-object burst should rewrite to exact EE shape");
    assert!(claim.add_records >= 1);
    assert!(claim.creature_appearance_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn unsupported_all_fields_appearance_does_not_fall_through_to_noop_claim() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/player_appearance_slot0_visible_equipment.bin"
    )
    .to_vec();
    // Corrupt the first visible-equipment add slot. The exact appearance parser
    // must reject it, and the narrow P/5 name-only no-op fallback must not claim
    // this complex 0xFFFF appearance mask with only three fragment bits.
    let first_visible_add_slot_in_payload = 7 + 157 + 5;
    payload[first_visible_add_slot_in_payload] = 0x77;
    assert!(super::claim_payload_if_verified(&payload).is_none());
}

#[test]
fn appearance_fixture_tail_cursor_anchor() {
    let payload = include_bytes!("../../../fixtures/live_object/player_appearance_false_u09.bin");
    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();
    let live = &payload[7..declared];
    let update_offset = 495;
    let update_end = live.len();
    let mut accepted = Vec::new();
    for start in super::CNW_FRAGMENT_HEADER_BITS..fragment_bits.len() {
        let mut cursor = start;
        if super::creature::advance_verified_noop_creature_update_record(
            live,
            update_offset,
            update_end,
            &fragment_bits,
            &mut cursor,
        ) {
            accepted.push((start, cursor));
        }
    }
    assert!(accepted.contains(&(29, 42)));
}

#[test]
fn creature_status_visibility_update_does_not_swallow_following_inventory_record() {
    let payload =
        include_bytes!("../../../fixtures/live_object/pending_live_object_seq31_chunks5.bin");
    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let live = &payload[7..declared];
    let update_offset = 597;
    let inventory_offset = 626;

    assert_eq!(&live[update_offset..update_offset + 2], &[b'U', 0x05]);
    assert_eq!(live[inventory_offset], b'I');
    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live,
            update_offset,
            live.len(),
        ),
        inventory_offset
    );

    let fragment_bits = vec![
        false, false, false, // CNW fragment length header.
        false, true, false, false, false, // 0x4000 first five BOOLs.
        false, false, // 0x4000 final two BOOLs.
        false, false, false, // 0x8000 self-visibility BOOLs.
    ];
    let mut stock_bit_cursor = 3;
    assert!(
        !super::creature::advance_verified_noop_creature_update_record(
            live,
            update_offset,
            inventory_offset,
            &fragment_bits,
            &mut stock_bit_cursor,
        ),
        "legacy c408 short status rows must be translated before EE exact validation"
    );
    assert_eq!(stock_bit_cursor, 3);

    let mut rewritten_live = live.to_vec();
    let mut rewritten_record_end = inventory_offset;
    let status_rewrite =
        super::creature::insert_creature_update_status_effect_identity_maps_for_ee(
            &mut rewritten_live,
            update_offset,
            &mut rewritten_record_end,
            &fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .expect("legacy c408 status rows should receive EE identity transform maps");
    assert_eq!(status_rewrite.entries, 3);
    assert_eq!(status_rewrite.bytes_inserted, 24);
    let mut translated_bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            &rewritten_live,
            update_offset,
            rewritten_record_end,
            &fragment_bits,
            &mut translated_bit_cursor,
        ),
        "translated c408 status rows should consume exactly before following inventory"
    );
    assert_eq!(translated_bit_cursor, fragment_bits.len());

    let mut rewritten_live = live.to_vec();
    rewritten_live[update_offset + 10] = 0;
    rewritten_live[update_offset + 11] = 0;
    let c408_rewrite = super::creature::repair_legacy_c408_visual_effect_count_for_ee(
        &mut rewritten_live,
        update_offset,
        inventory_offset,
    )
    .expect("HG malformed c408 zero-count capture should be normalized before EE validation");
    assert_eq!(c408_rewrite.entries, 3);
    assert_eq!(c408_rewrite.bytes_rewritten, 2);
    assert_eq!(
        &rewritten_live[update_offset + 10..update_offset + 12],
        &[3, 0]
    );
    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    let mut rewritten_record_end = inventory_offset;
    let status_rewrite =
        super::creature::insert_creature_update_status_effect_identity_maps_for_ee(
            &mut rewritten_live,
            update_offset,
            &mut rewritten_record_end,
            &fragment_bits,
            bit_cursor,
        )
        .expect("repaired c408 rows should receive EE identity transform maps");
    assert_eq!(status_rewrite.entries, 3);
    assert_eq!(status_rewrite.bytes_inserted, 24);
    assert!(
        super::creature::advance_verified_noop_creature_update_record(
            &rewritten_live,
            update_offset,
            rewritten_record_end,
            &fragment_bits,
            &mut bit_cursor,
        )
    );
    assert_eq!(bit_cursor, fragment_bits.len());
}

#[test]
fn inventory_delta_followed_by_gui_item_add_rewrites_and_claims_exactly() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/player_hide_inventory_gui_span.bin").to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("embedded GUI item-create active-property bit should be rewritten");
    assert_eq!(rewrite.bits_inserted, 1);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("inventory delta plus GIA item-create should be exactly claimed");
    assert_eq!(claim.records_examined, 2);
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 1);
    // EE `sub_14076BD30` consumes the same two item-name selector bits as the
    // legacy inline-locstring branch, then one shared pre-DWORD active-property
    // BOOL plus four post-DWORD BOOLs. Diamond `sub_451020` has only three
    // post-DWORD BOOLs, so the translated GUI item-create owns seven fragment
    // bits after insertion.
    assert_eq!(claim.live_gui_fragment_bits, 7);
}

#[test]
fn inventory_0400_delta_requires_exact_fragment_cursor_consumption() {
    let live = [
        b'I', 0xDC, 0xFF, 0xFF, 0xFF, // live inventory owner id
        0x00, 0x04, // mask 0x0400: clear slots, then set slots
        0x01, 0x6B, // one cleared slot
        0x01, 0x6B, // one set slot, therefore one CNW fragment BOOL
    ];

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false, false, false, true],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim =
        super::claim_payload_if_verified(&payload).expect("exact 0x0400 inventory delta claim");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 1);

    let mut payload_with_extra_fragment_bit = vec![b'P', 0x05, 0x01];
    payload_with_extra_fragment_bit.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload_with_extra_fragment_bit.extend_from_slice(&live);
    payload_with_extra_fragment_bit.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false, false, false, true, false],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(super::claim_payload_if_verified(&payload_with_extra_fragment_bit).is_none());
}

#[test]
fn inventory_0401_delta_requires_false_compact_state_bool_before_equipment_delta() {
    // Live HG sequence 38 begins with this player-owned inventory state record
    // before the Town Greeter/Northern Trader creature records:
    //
    //   I ECFFFFFF 0104 9500 D4D9E005 EB0A0000 02 1E6B 01 6B
    //
    // EE `sub_1407B4F70` (`1407B51ED..1407B559F`) and Diamond `sub_455940`
    // (`00455AAD..00455D80`) both consume the 0x0001 branch as SHORT, DWORD,
    // INT, BOOL. If that BOOL is true, both readers immediately consume an
    // extended tail starting with another WORD before the 0x0400 equipment
    // delta. This compact cursor is therefore exact only when the BOOL is
    // false.
    let live = [
        b'I', 0xEC, 0xFF, 0xFF, 0xFF, // player inventory owner id
        0x01, 0x04, // mask 0x0401: 0x0001 state + 0x0400 equipment delta
        0x95, 0x00, // 0x0001 state SHORT
        0xD4, 0xD9, 0xE0, 0x05, // 0x0001 state DWORD
        0xEB, 0x0A, 0x00, 0x00, // 0x0001 state INT
        0x02, 0x1E, 0x6B, // 0x0400 clear-count and clear slots
        0x01, 0x6B, // 0x0400 set-count and set slot
    ];

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            false, // 0x0001 compact-state BOOL: no extended tail
            true,  // 0x0400 set-slot BOOL
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim = super::claim_payload_if_verified(&payload)
        .expect("0x0401 inventory delta should accept the compact false state BOOL");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 2);

    let mut shifted_payload = vec![b'P', 0x05, 0x01];
    shifted_payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    shifted_payload.extend_from_slice(&live);
    shifted_payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            true,  // 0x0001 extended-tail selector
            true,  // would be misread as 0x0400 set-slot BOOL by the old bypass
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(
        super::claim_payload_if_verified(&shifted_payload).is_none(),
        "a true 0x0001 BOOL must not compact-hand off directly to the 0x0400 delta"
    );
}

#[test]
fn inventory_2700_zero_count_branch_requires_matching_fragment_bool() {
    let live = [
        b'I', 0xB0, 0xFF, 0xFF, 0xFF, // inventory/GUI owner id
        0x00, 0x27, // mask 0x2700: 0x0400, 0x0200, 0x0100, 0x2000
        0x00, 0x01, 0x6B, // 0x0400: no clears, one set slot
        0x00, 0x00, 0x00, 0x00, // 0x0200: zero-count DWORD branch
        0x00, // 0x0100: empty opcode stream
        0x00, 0x00, 0x00, 0x00, // 0x2000: first object-list count
        0x00, 0x00, 0x00, 0x00, // 0x2000: second object-list count
    ];

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            true,  // 0x0400 set-slot BOOL
            false, // 0x0200 first branch BOOL; does not affect read cursor here
            false, // 0x0200 second branch BOOL: false selects the DWORD branch
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    let claim = super::claim_payload_if_verified(&payload)
        .expect("0x2700 zero-count DWORD branch should validate with matching BOOLs");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 3);

    let mut payload_with_wrong_branch_bit = vec![b'P', 0x05, 0x01];
    payload_with_wrong_branch_bit.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload_with_wrong_branch_bit.extend_from_slice(&live);
    payload_with_wrong_branch_bit.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            true,  // 0x0400 set-slot BOOL
            false, // 0x0200 first branch BOOL
            true,  // 0x0200 second branch BOOL: true selects the byte-mask branch
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(
        super::claim_payload_if_verified(&payload_with_wrong_branch_bit).is_none(),
        "the byte cursor alone is not an exact proof when the 0x0200 BOOL selects another branch"
    );
}

#[test]
fn local_diamond_inventory_0200_counted_cells_require_exact_branch_bits() {
    // Local Diamond door/placeable click capture from 2026-05-18:
    //
    //   I FFFEFFFF 0002 03000000 0202 0302 0402 | 07
    //
    // Diamond `sub_455940` and EE `sub_1407B4F70` both read mask 0x0200 as
    // two CNW BOOLs followed by a branch.  For this capture the branch bits
    // are false/false, so the read-buffer body is DWORD count plus count
    // two-byte cells, and the fragment stream owns one BOOL per cell.
    let payload = local_diamond_inventory_0200_payload(&[(2, 2), (3, 2), (4, 2)]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("local Diamond I/0x0200 counted-cell inventory should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 5);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);

    let fragment_offset = u32::from_le_bytes(payload[3..7].try_into().unwrap()) as usize;
    let mut wrong_branch = payload.clone();
    wrong_branch[fragment_offset] |= 0x08;
    assert!(
        super::claim_payload_if_verified(&wrong_branch).is_none(),
        "0x0200 counted-cell branch must require the decompiled second BOOL=false path"
    );
}

#[test]
fn local_diamond_inventory_0200_counted_cells_second_capture_claims_exactly() {
    let payload = local_diamond_inventory_0200_payload(&[(1, 2), (1, 3), (1, 4)]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("second local Diamond I/0x0200 counted-cell inventory should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 5);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn inventory_2000_zero_count_trailing_pair_keeps_following_live_boundary() {
    let live = [
        b'I', 0xAD, 0xFF, 0xFF, 0xFF, // inventory owner object id
        0x00, 0x20, // mask 0x2000: feature-25 inventory object lists
        0x00, 0x00, 0x00, 0x00, // first list count
        0x00, 0x00, 0x00, 0x00, // second list count
        0x8B, 0x75, 0x01, 0x80, // captured Diamond compatibility tail object
        0x91, 0x75, 0x01, 0x80, // captured Diamond compatibility tail object
        b'U', 0x05, 0x9D, 0x75, 0x01, 0x80, // next live-object update boundary
        0x00, 0x00, 0x00, 0x00,
    ];

    assert_eq!(
        super::inventory::try_get_legacy_live_inventory_fragment_bit_count(&live, 0, 23),
        Some(0),
        "zero-count 0x2000 legacy compatibility tail owns no fragment BOOLs"
    );
    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            &live,
            0,
            live.len(),
        ),
        23,
        "validated inventory 0x2000 tail must not swallow the following U/5 record"
    );
}

fn local_diamond_inventory_0200_payload(cells: &[(u8, u8)]) -> Vec<u8> {
    let mut live = vec![
        b'I', 0xFE, 0xFF, 0xFF, 0xFF, // sentinel/self inventory owner id
        0x00, 0x02, // mask 0x0200
    ];
    live.extend_from_slice(&(cells.len() as u32).to_le_bytes());
    for (x, y) in cells {
        live.push(*x);
        live.push(*y);
    }

    let mut payload = vec![b'P', 0x05, 0x01];
    payload.extend_from_slice(&((7 + live.len()) as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![
            false, false, false, // CNW fragment header bits
            false, // 0x0200 first branch BOOL: counted cells carry per-cell BOOLs
            false, // 0x0200 second branch BOOL: false selects DWORD-count body
            true, true, true, // one semantic cell BOOL for each counted cell
        ],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));
    payload
}

#[test]
fn placeable_looping_effect_updates_expand_identity_visual_maps() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/placeable_effect_list_two_records_legacy.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("legacy placeable looping-effect update list should expand for EE");
    assert_eq!(rewrite.update_records_rewritten, 2);
    assert_eq!(rewrite.bytes_inserted, 16);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("expanded looping-effect updates should validate exactly");
    assert_eq!(claim.records_examined, 2);
    assert_eq!(claim.update_records, 2);
}

#[test]
fn creature_update_mask_0047_is_not_rewritten_as_visual_transform_selector() {
    let mut live = vec![
        b'U', 0x05, 0x82, 0x67, 0x01, 0x80, // creature object id
        0x47, 0x00, 0x00, 0x00, // decompile-supported creature update mask
    ];
    live.extend_from_slice(&[
        0x00, 0x00, 0x80, 0x3F, // plausible read-buffer body bytes
        0x00, 0x00, 0x80, 0x3F,
    ]);
    let mut fragment_bits = vec![false, false, false, false, true, false, false, false];
    let mut record_end = live.len();

    assert!(
        super::appearance::rewrite_creature_visual_transform_update_for_ee(
            &mut live,
            0,
            &mut record_end,
            &mut fragment_bits,
            super::CNW_FRAGMENT_HEADER_BITS,
        )
        .is_none()
    );
    assert_eq!(&live[6..10], &[0x47, 0x00, 0x00, 0x00]);
}

#[test]
fn quarantined_creature_update_mask_0047_claims_exactly() {
    let payload =
        include_bytes!("../../../fixtures/live_object/creature_update_mask_0047_direct.bin");

    let claim = super::claim_payload_if_verified(payload)
        .expect("decompile-shaped U/5 mask 0x47 update should be exactly claimed");
    assert_eq!(claim.records_examined, 1);
    assert_eq!(claim.creature_update_records, 1);
    assert_eq!(claim.live_bytes_length, 36);
    assert_eq!(claim.fragment_bytes, 2);
}

#[test]
fn driver_direct_creature_update_mask_0047_variants_claim_exactly() {
    // Driver-only HG captures from the Starcore5 run after area load. Diamond
    // and EE both walk these `U/5` creature movement/status records through the
    // same mask-ordered reader branches modelled by `creature.rs`: position,
    // orientation, action, and branch-0x40 data. These fixtures are identity
    // translations only after the bounded creature cursor consumes the record
    // and its CNW fragment bits exactly; they must not fall through to the
    // generic door/placeable update path or the legacy visual-transform shim.
    let cases: &[(&[u8], usize)] = &[
        (
            include_bytes!(
                "../../fixtures/live_object/hg_direct_creature_update_0047_move_idle_33.bin"
            ),
            32,
        ),
        (
            include_bytes!(
                "../../fixtures/live_object/hg_direct_creature_update_0047_move_action2_37.bin"
            ),
            36,
        ),
        (
            include_bytes!(
                "../../fixtures/live_object/hg_direct_creature_update_0047_move_action2_49.bin"
            ),
            48,
        ),
    ];

    for (idx, (payload, live_bytes)) in cases.iter().enumerate() {
        let claim = super::claim_payload_if_verified(payload).unwrap_or_else(|| {
            panic!(
                "driver-captured U/5 mask 0x47 movement/status update case {idx} should claim exactly"
            )
        });
        assert_eq!(claim.records_examined, 1);
        assert_eq!(claim.creature_update_records, 1);
        assert_eq!(claim.update_records, 0);
        assert_eq!(claim.creature_visual_transform_update_records, 0);
        assert_eq!(claim.live_bytes_length, *live_bytes);
        assert_eq!(claim.fragment_bytes, 2);
    }
}

#[test]
fn driver_direct_creature_update_mask_0040_then_0047_pair_claims_exactly() {
    // Starcore5 driver-only capture after area load:
    //
    //   U/5 mask 0x40: compact creature state branch, mode 1, one fragment BOOL.
    //   U/5 mask 0x47: movement/action/state branch already modelled above.
    //
    // Diamond/EE consume the first record at the exact branch-0x40 read cursor.
    // The following `U` byte is a real live-object boundary, not inline string
    // data, so the boundary scanner must classify 0x40 as a numeric creature
    // update before the exact creature cursor validates both records.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_direct_creature_update_0040_then_0047_pair.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("driver-captured U/5 0x40 + U/5 0x47 pair should claim exactly");
    assert_eq!(claim.records_examined, 2);
    assert_eq!(claim.creature_update_records, 2);
    assert_eq!(claim.update_records, 0);
    assert_eq!(claim.live_bytes_length, 48);
    assert_eq!(claim.fragment_bytes, 2);
}

#[test]
fn legacy_creature_visual_transform_selector_still_gets_identity_map() {
    let mut live = vec![
        b'U', 0x05, 0x82, 0x67, 0x01, 0x80, // creature object id
        0x01, // legacy selector byte, not a four-byte creature update mask
    ];
    let mut fragment_bits = vec![false, false, false];
    let mut record_end = live.len();

    let rewrite = super::appearance::rewrite_creature_visual_transform_update_for_ee(
        &mut live,
        0,
        &mut record_end,
        &mut fragment_bits,
        super::CNW_FRAGMENT_HEADER_BITS,
    )
    .expect("selector-only legacy visual-transform update should rewrite");

    assert_eq!(rewrite.bytes_inserted, 8);
    assert_eq!(rewrite.bytes_removed, 0);
    assert_eq!(record_end, 15);
    assert!(
        super::appearance::is_verified_ee_creature_visual_transform_update_record(
            &live, 0, record_end
        )
    );
}

#[test]
fn captured_creature_pair_3967_short_stream_rewrites_to_exact_ee_shape() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/creature_pair_3967_short_stream.bin")
            .to_vec();

    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured A/5 + P/5 + U/5 0x3967 stream should rewrite");
    assert_eq!(summary.records_examined, 7);
    assert_eq!(summary.bytes_inserted, 199);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten captured creature stream should validate exactly");
    assert_eq!(claim.add_records, 2);
    assert_eq!(claim.creature_appearance_records, 2);
    assert_eq!(claim.creature_update_records, 2);
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.read_buffer_only_records, 0);
}

#[test]
fn hg_starc5_sooty_3967_action0_post_state_count_preserves_ee_shape() {
    let raw = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_transition_3967_action0_bridge_followup_20260514.bin"
    );
    assert!(
        super::claim_payload_if_verified(raw).is_none(),
        "raw stream still needs the surrounding A/P/tail rewrite before exact EE validation"
    );

    let mut payload = raw.to_vec();
    let old_declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("legacy declared length");
    let summary = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("0x3967 action-0 stream rewrite");
    assert_eq!(summary.update_records_rewritten, 0);
    assert_eq!(summary.bytes_inserted, 29);
    assert_eq!(summary.bytes_removed, 44);

    let new_declared = super::read_u32_le(&payload, super::HIGH_LEVEL_HEADER_BYTES)
        .expect("rewritten declared length");
    assert_eq!(new_declared, old_declared - 15);
    assert_eq!(payload.len(), raw.len() - 15);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten 0x3967 action-0 stream should validate exactly");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 1);
    assert_eq!(claim.world_status_records, 1);
    assert_eq!(claim.read_buffer_only_records, 0);

    let marker = [b'U', 0x05, 0xF7, 0x4C, 0x01, 0x80, 0x67, 0x39, 0x00, 0x00];
    let u_offset = raw
        .windows(marker.len())
        .position(|window| window == marker)
        .expect("captured 0x3967 update marker");
    let mut buggy_missing_state_byte = raw.to_vec();
    super::write_u32_le(
        &mut buggy_missing_state_byte,
        super::HIGH_LEVEL_HEADER_BYTES,
        old_declared - 3,
    )
    .expect("adjust buggy declared length");
    buggy_missing_state_byte.drain(u_offset + 25..u_offset + 28);
    assert!(
        super::claim_payload_if_verified(&buggy_missing_state_byte).is_none(),
        "removing the EE action-state byte plus follow-up WORD recreates the crash shape"
    );
}

#[test]
fn area_entry_door_pairs_rewrite_to_exact_ee_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_area_entry_door_pairs_claimed_records.bin"
    )
    .to_vec();

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 2);
    assert!(claim.update_records >= 2);
}

#[test]
fn area_entry_door_and_signs_rewrite_to_exact_ee_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_area_entry_door_signs_mixed_liveobject.bin"
    )
    .to_vec();

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 7);
    assert!(claim.update_records >= 7);
}

#[test]
fn trigger_add_mentions_expose_verified_geometry_bounds() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/hg_seq29_trigger_door_mixed_add_update.bin")
            .to_vec();

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    let trigger = claim
        .mentions
        .iter()
        .find(|mention| mention.opcode == b'A' && mention.object_type == super::TRIGGER_OBJECT_TYPE)
        .expect("exact claim should expose the verified trigger add mention");
    let bounds = trigger
        .bounds
        .expect("verified trigger geometry should derive finite protocol bounds");
    assert!(bounds.min_x.is_finite());
    assert!(bounds.min_y.is_finite());
    assert!(bounds.min_z.is_finite());
    assert!(bounds.max_x.is_finite());
    assert!(bounds.max_y.is_finite());
    assert!(bounds.max_z.is_finite());
    assert!(bounds.min_x <= bounds.max_x);
    assert!(bounds.min_y <= bounds.max_y);
    assert!(bounds.min_z <= bounds.max_z);
}

#[test]
fn hg_seq41_creature_captain_mixed_add_update_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_seq41_creature_captain_mixed_add_update.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("HG Captain mixed creature appearance/update stream should claim exactly");
    assert!(claim.creature_appearance_records >= 1);
    assert!(claim.creature_update_records >= 1);
}

#[test]
fn captured_hg_self_c408_inventory_stream_is_claimed_after_boundary_floor() {
    let mut payload =
        include_bytes!("../../../fixtures/live_object/hg_self_c408_inventory_unclaimed.bin")
            .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload);
    assert!(
        rewrite.is_some(),
        "captured HG C408 self/status stream should rewrite through the exact live-object path"
    );

    let claim = super::claim_payload_if_verified(&payload);
    assert!(
        claim.is_some(),
        "rewritten HG C408 self/status stream should be exactly claimable"
    );
}

#[test]
fn local_diamond_bw167demo_u5_4408_inventory_stream_rewrites_to_exact_shape() {
    // Local Diamond server harness capture from bw167demo on 2026-05-16. The
    // stream starts with `U/5 0x00004408`: a compact legacy status-effect delta,
    // four scalar/status WORDs, and the fragment-only self/status suffix, then
    // immediately continues with an `I` inventory record. The interior status
    // effect opcode byte is `A`, so the generic live-object boundary scanner must
    // defer to the decompile-backed creature cursor instead of splitting early.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_bw167demo_u5_4408_inventory_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("local Diamond 0x4408 status stream should rewrite through creature translator");
    assert!(
        rewrite.update_records_rewritten >= 1 || rewrite.bytes_inserted > 0,
        "0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local Diamond 0x4408 stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
}

#[test]
fn local_diamond_auto_inventory_u5_4408_gui_rows_stream_rewrites_to_exact_shape() {
    // Local Diamond harness capture from 2026-05-19 after the driver opened
    // inventory immediately after Area_AreaLoaded. It starts with the same
    // `U/5 0x00004408` compact creature status family as the shorter bw167demo
    // fixture, then continues with the GUI inventory/repository row block
    // emitted by the inventory panel. The live-object stream owner must keep
    // this whole packet in the typed 0x4408 + GUI-row path instead of buffering
    // it as an unclaimed continuation.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq18_auto_inventory_u5_4408_gui_rows_20260519_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw local Diamond auto-inventory stream lacks the exact EE live-object opcode shape"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("local Diamond auto-inventory 0x4408 stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1
            || rewrite.bytes_inserted > 0
            || rewrite.bytes_removed > 0
            || rewrite.interleaved_fragment_spans_promoted > 0,
        "auto-inventory 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local Diamond auto-inventory stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(
        claim.live_gui_item_create_records >= 1,
        "GUI inventory/repository rows should remain owned by exact live-object claim"
    );
}

#[test]
fn local_to_heir_u5_4408_inventory_2a00_word_list_rewrites_to_exact_shape() {
    // Local To Heir harness capture from 2026-05-19 after Area_ClientArea was
    // repaired from the module ARE. The stream starts with the decompile-owned
    // compact `U/5 0x00004408` self/status record, then an `I/0x2A00` current
    // player inventory body. This module takes the 0x0200 false branch as a
    // nonzero DWORD count followed by WORD entries before the Feature-25 lists.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_to_heir_seq16_u5_4408_inventory_2a00_20260519_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw To Heir 0x4408 + 0x2A00 stream is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("To Heir 0x4408 + inventory stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1 || rewrite.bytes_inserted > 0,
        "To Heir 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten To Heir inventory stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
}

#[test]
fn local_to_heir_kraegen_thoraulik_live_object_stream_rewrites_to_exact_shape() {
    // Local To Heir creature auto-use/dialog harness capture from 2026-05-24.
    // The stream starts with a current-player update and then interleaves
    // Kraegen/Thoraulik creature add/update records plus inventory/read-buffer
    // state. The raw Diamond payload is intentionally kept as an unclaimed
    // fixture so the bounded exact adapter must own the full rewrite.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_to_heir_seq19_kraegen_thoraulik_liveobject_20260524_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw To Heir Kraegen/Thoraulik stream should document the unclaimed Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.creature_appearance_records >= 1);
    assert!(claim.creature_update_records >= 2);
    assert!(
        claim.mentions.iter().any(|mention| {
            mention.opcode == b'U' && mention.object_type == super::CREATURE_OBJECT_TYPE
        }),
        "creature update records should remain owned by exact live-object claim"
    );
    assert_no_full_creature_appearance_has_longer_legacy_shape(&payload);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter2_current_player_appearance_preserves_full_body_semantics() {
    // Local Chapter2 bridge run from 2026-05-24, captured after the visual
    // alignment report. This current-player stream is a semantic regression
    // seed: byte-shape acceptance alone is not enough because the source BIC
    // and legacy P/5 record both prove Appearance_Type=2, while a shifted
    // rewrite can stop the full appearance before its body/equipment table.
    let legacy = include_bytes!(
        "../../../fixtures/live_object/local_chapter2_current_player_appearance_20260524_legacy.bin"
    );
    let stale_ee = include_bytes!(
        "../../../fixtures/live_object/local_chapter2_current_player_appearance_20260524_ee.bin"
    );

    let legacy_semantics = current_player_full_appearance_semantics(
        legacy,
        TestCreatureAppearanceDialect::LegacyDiamond,
    )
    .expect("legacy current-player P/5 should expose full appearance semantics");
    assert_eq!(legacy_semantics.object_id, 0xFFFF_FFFE);
    assert_eq!(
        legacy_semantics.appearance_type, 2,
        "legacy current-player Appearance_Type should match the seed BIC"
    );
    assert_eq!(legacy_semantics.body_selector, 0x13);
    assert_eq!(legacy_semantics.equipment_count, 8);

    assert!(
        super::claim_payload_if_verified(stale_ee).is_none(),
        "stale EE diagnostic stopped the P/5 record before the full body/equipment table"
    );
    assert!(
        current_player_full_appearance_semantics(
            stale_ee,
            TestCreatureAppearanceDialect::EeBuild8193,
        )
        .is_none(),
        "stale EE diagnostic must not satisfy the semantic current-player appearance proof"
    );

    let mut payload = legacy.to_vec();
    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.creature_appearance_records >= 1);
    assert_ne!(
        payload.as_slice(),
        stale_ee,
        "rewriter must not reproduce the stale shifted EE current-player appearance"
    );

    let ee_semantics = current_player_full_appearance_semantics(
        &payload,
        TestCreatureAppearanceDialect::EeBuild8193,
    )
    .expect("rewritten current-player P/5 should expose full EE appearance semantics");
    assert_eq!(
        ee_semantics.appearance_type,
        legacy_semantics.appearance_type
    );
    assert_eq!(ee_semantics.body_selector, legacy_semantics.body_selector);
    assert_eq!(
        ee_semantics.equipment_count,
        legacy_semantics.equipment_count
    );
    assert!(
        ee_semantics.record_end > legacy_semantics.record_end,
        "EE build-0x23 widening should retain, not truncate, the full appearance body"
    );
    assert_no_full_creature_appearance_has_longer_legacy_shape(&payload);
}

#[test]
fn local_xp1_u5_4408_inventory_2a00_single_word_list_rewrites_to_exact_shape() {
    // Local XP1-Chapter 1 harness capture from 2026-05-22. The stream starts
    // with compact `U/5 0x00004408`, then current-player `I/0x2A00` where the
    // 0x0200 branch uses a single WORD list entry before Feature-25 and the
    // 0x0800 true tail. This is the same decompiled reader order as the wider
    // To Heir fixture, but with the shortest observed nonzero WORD-list body.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp1_current_player_4408_live_object_20260522.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw XP1 0x4408 + 0x2A00 stream is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("XP1 0x4408 + inventory stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1 || rewrite.bytes_inserted > 0,
        "XP1 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten XP1 current-player inventory stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp1_chapter2_4408_inventory_creature_stream_rewrites_to_exact_shape() {
    // Local XP1-Chapter 2 harness capture from 2026-05-24. The stream starts
    // with `U/5 0x00004408`, but this module emits two counted visual-effect
    // entries before the current-player `I/0x2A00` inventory/read-buffer block
    // and the following Merom Rescher creature add/update records. The clean
    // rerun dumped both the raw Diamond payload and the exact EE writer output,
    // so pin the byte shape rather than widening strict validation.
    let legacy = include_bytes!(
        "../../../fixtures/live_object/local_xp1_chapter2_seq16_4408_inventory_creature_20260524_legacy.bin"
    )
    .as_slice();
    let mut payload = legacy.to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_xp1_chapter2_seq16_4408_inventory_creature_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw XP1-Chapter 2 0x4408 + inventory/creature stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_rewrite_matches_expected_or_current_player_semantics(
        "XP1-Chapter 2 0x4408",
        legacy,
        payload.as_slice(),
        expected_ee,
        "XP1-Chapter 2 0x4408 rewrite should match the harness-dumped EE bytes",
    );
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.add_records >= 1);
    assert!(
        claim.live_gui_read_buffer_records >= 1,
        "XP1-Chapter 2 stream should retain GUI/read-buffer ownership after rewrite"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp1_chapter1_area_entry_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local XP1-Chapter 1 harness run from 2026-05-24. The accepted
    // live-object diagnostic dumped the area-entry Diamond payloads and the
    // exact EE writer output, so these fixtures pin the bounded writer byte
    // shape instead of broadening validation around this module family.
    for (name, legacy, expected_ee) in [
        (
            "seq13",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq13_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq13_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq14",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq14_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq14_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq15",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq15_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq15_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq16",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq16_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_chapter1_seq16_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw XP1-Chapter 1 stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE bytes"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain at least one typed live-object record"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp1_chapter1_auto_inventory_gui_stream_matches_dumped_ee_shape() {
    // Same XP1-Chapter 1 run after auto-opening inventory. This compact GIA/GRA
    // live-object stream must remain owned by the exact GUI-row validator.
    let legacy = include_bytes!(
        "../../../fixtures/live_object/local_xp1_chapter1_seq26_auto_inventory_gui_20260524_legacy.bin"
    )
    .as_slice();
    let mut payload = legacy.to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_xp1_chapter1_seq26_auto_inventory_gui_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw XP1-Chapter 1 auto-inventory GUI stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_rewrite_matches_expected_or_current_player_semantics(
        "XP1-Chapter 1 auto-inventory GUI",
        legacy,
        payload.as_slice(),
        expected_ee,
        "XP1-Chapter 1 auto-inventory GUI rewrite should match the harness-dumped EE bytes",
    );
    assert!(
        claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
        "XP1-Chapter 1 auto-inventory stream should retain GUI live-object row ownership"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp1_interlude_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local XP1-Interlude `x1_premonition` strict-clean harness run from
    // 2026-05-24. The accepted-live-object diagnostics captured both the
    // Diamond inputs and the exact EE writer outputs for area-entry, dialog
    // heartbeat, and auto-inventory streams.
    for (name, legacy, expected_ee, expect_gui) in [
        (
            "seq12",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq12_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq12_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq13",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq13_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq13_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq14",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq14_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq14_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq15",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq15_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq15_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq16",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq16_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq16_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq17",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq17_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq17_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq18",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq18_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq18_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq21",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq21_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq21_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq30_auto_inventory_gui",
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq30_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp1_interlude_seq30_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            true,
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw XP1-Interlude stream should document the legacy Diamond shape"
        );

        let started = std::time::Instant::now();
        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "{name} XP1-Interlude live-object rewrite must stay bounded"
        );
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE bytes"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain at least one typed live-object record"
        );
        if expect_gui {
            assert!(
                claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                "{name} should retain GUI live-object row ownership"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[test]
fn local_xp2_chapter2_current_player_4408_inventory_2a00_compact_rewrites_to_exact_shape() {
    // Local XP2 Chapter 2 harness capture from 2026-05-22 after the inventory
    // panel was opened in the starting cutscene. It uses the same compact
    // `U/5 0x00004408` current-player status record as earlier local captures,
    // but the following `I/0x2A00` row takes the byte-mask branch with five
    // mask bytes before a compact Feature-25/current-player body.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_chapter2_current_player_4408_inventory_2a00_compact_20260522.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw XP2 Chapter 2 compact 0x4408 + 0x2A00 stream is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("XP2 Chapter 2 compact 0x4408 + 0x2A00 stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1 || rewrite.bytes_inserted > 0,
        "XP2 Chapter 2 compact 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten XP2 Chapter 2 compact inventory stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
}

#[test]
fn local_dark_ranger_u5_effect_inventory_gui_stream_rewrites_to_exact_shape() {
    // Local Dark Ranger harness capture from 2026-05-19 after Module_Info and
    // Area_ClientArea were repaired. The live stream starts with a compact
    // `U/5 0x00000008` LowLightVision effect row, then carries current-player
    // inventory/GUI rows and the innkeeper creature add/update records. The
    // bytes between the effect row and `I` boundary must be owned by the same
    // decompile-backed CNW fragment-storage proof used by the exact final
    // validator, not forwarded as live-object read bytes.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_dark_ranger_seq13_live_object_u5_inventory_20260519_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Dark Ranger 0x0008 + inventory stream is still legacy-shaped"
    );

    crate::translate::live_object::normalize_prefixed_fragments_payload_if_needed(&mut payload)
        .expect("Dark Ranger raw prefixed-fragment envelope should normalize");
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("Dark Ranger 0x0008 + inventory stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1
            || rewrite.bytes_inserted > 0
            || rewrite.interleaved_fragment_spans_promoted > 0,
        "Dark Ranger 0x0008 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten Dark Ranger stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
    assert!(claim.add_records >= 1);
}

#[test]
fn local_dark_ranger_seq15_4408_inventory_gui_stream_rewrites_to_exact_shape() {
    // Local Dark Ranger harness capture from 2026-05-23 after the BNK2 local
    // transport delay was fixed. The packet is already a full declared
    // `P/05/01` live-object payload and starts with the compact Diamond
    // current-player `U/5 0x4408` effect/status reader before the same
    // inventory/GUI/innkeeper add-update family as the older Dark Ranger
    // fixture.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_dark_ranger_seq15_u5_4408_inventory_gui_20260523_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Dark Ranger seq15 stream is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("Dark Ranger seq15 0x4408 + inventory stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1
            || rewrite.bytes_inserted > 0
            || rewrite.interleaved_fragment_spans_promoted > 0,
        "Dark Ranger seq15 0x4408 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten Dark Ranger seq15 stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
    assert!(claim.add_records >= 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_dark_ranger_seq18_auto_inventory_gui_stream_matches_dumped_ee_shape() {
    // Local Dark Ranger harness run from 2026-05-24 after auto-opening
    // inventory. The accepted-live-object diagnostic dumped both the raw
    // Diamond GIA/GRA GUI stream and the exact EE rewrite, so this fixture pins
    // byte-for-byte ownership without widening the live-object validator.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_dark_ranger_seq18_auto_inventory_gui_20260524_legacy.bin"
    )
    .to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_dark_ranger_seq18_auto_inventory_gui_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Dark Ranger seq18 GUI stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(
        payload.as_slice(),
        expected_ee,
        "Dark Ranger seq18 rewrite should match the harness-dumped EE bytes"
    );
    assert!(
        claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
        "GUI live-object rows should remain owned by the exact validator"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_cepv23_skies_auto_inventory_gui_stream_matches_dumped_ee_shape() {
    // Local CEP v2.3 skies harness run from 2026-05-24 after auto-opening
    // inventory. This compact GIA/GRA stream is the same packet family as the
    // Dark Ranger and Winds GUI captures, but carries different object/body
    // bytes; keep it pinned to the exact accepted-live-object rewrite.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_cepv23_skies_seq17_auto_inventory_gui_20260524_legacy.bin"
    )
    .to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_cepv23_skies_seq17_auto_inventory_gui_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw CEP v2.3 skies GUI stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(
        payload.as_slice(),
        expected_ee,
        "CEP v2.3 skies rewrite should match the harness-dumped EE bytes"
    );
    assert!(
        claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
        "GUI live-object rows should remain owned by the exact validator"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_cepv22_starter_area_entry_live_object_rewrites_to_exact_shape() {
    // Local CEP v2.2 starter harness run from 2026-05-24. The proxy reached
    // this area-entry live-object stream while loading the small CEP starter
    // area; keep it as private evidence for the bounded exact live-object
    // adapter rather than broadening the dispatcher around a module-specific
    // shape.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_cepv22_starter_seq12_liveobject_20260524_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw CEP v2.2 starter stream should document the legacy Diamond shape"
    );

    let mut with_area_context = payload.clone();
    let area_context = crate::translate::area::AreaPlaceableContext {
        area_resref: "area".to_string(),
        static_rows: vec![crate::translate::area::AreaPlaceableContextRow {
            object_id: 0x8000_006D,
            appearance: 0x2743,
            x: 1.0,
            y: 1.0,
            z: 0.0,
            dir_x: 0.0,
            dir_y: 1.0,
            dir_z: 0.0,
            has_direction: true,
            module_state: None,
        }],
        light_rows: Vec::new(),
    };
    assert!(
        crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
            &mut with_area_context,
            Some(&area_context)
        ),
        "area-backed exact rewrite should remain bounded for the CEP v2.2 starter stream"
    );
    let area_context_claim = super::claim_payload_if_verified(&with_area_context)
        .expect("area-backed rewrite should exact-claim");
    assert!(area_context_claim.add_records >= 1);

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 1);
    assert!(
        claim.creature_appearance_records + claim.creature_update_records >= 1,
        "CEP v2.2 starter rewrite should own at least one creature record"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_shadowguard_auto_inventory_gui_stream_matches_dumped_ee_shape() {
    // Local ShadowGuard premium module harness run from 2026-05-24 after
    // auto-opening inventory. The bytes match the CEPv23 compact GUI family,
    // but this fixture pins the module-specific accepted-live-object evidence.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_shadowguard_seq18_auto_inventory_gui_20260524_legacy.bin"
    )
    .to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_shadowguard_seq18_auto_inventory_gui_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw ShadowGuard GUI stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(
        payload.as_slice(),
        expected_ee,
        "ShadowGuard rewrite should match the harness-dumped EE bytes"
    );
    assert!(
        claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
        "GUI live-object rows should remain owned by the exact validator"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_kingmaker_auto_inventory_gui_stream_matches_dumped_ee_shape() {
    // Local Kingmaker premium module harness run from 2026-05-24 after
    // auto-opening inventory. The bytes match the ShadowGuard compact GUI
    // family, but this fixture pins the module-specific accepted-live-object
    // evidence from the same run that supplied the Kingmaker quickbar fixture.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_kingmaker_seq17_auto_inventory_gui_20260524_legacy.bin"
    )
    .to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_kingmaker_seq17_auto_inventory_gui_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Kingmaker GUI stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(
        payload.as_slice(),
        expected_ee,
        "Kingmaker rewrite should match the harness-dumped EE bytes"
    );
    assert!(
        claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
        "GUI live-object rows should remain owned by the exact validator"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_witchs_wake_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local Witch's Wake premium module harness run from 2026-05-24 reached
    // gameplay and auto-opened inventory. The accepted-live-object diagnostics
    // captured the bounded area-entry rewrite and the later compact
    // current-player inventory update as exact legacy/EE pairs.
    for (name, legacy, expected_ee, expect_inventory) in [
        (
            "seq13_area_entry",
            include_bytes!(
                "../../../fixtures/live_object/local_witchs_wake_seq13_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_witchs_wake_seq13_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq15_auto_inventory",
            include_bytes!(
                "../../../fixtures/live_object/local_witchs_wake_seq15_auto_inventory_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_witchs_wake_seq15_auto_inventory_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            true,
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw Witch's Wake stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE bytes"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain typed live-object record ownership"
        );
        if expect_inventory {
            assert!(
                claim.inventory_records >= 1,
                "{name} should retain current-player inventory ownership"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn hg_live_seq42_auto_inventory_gui_stream_matches_dumped_ee_shape() {
    // Live HG smoke run from 2026-05-24 after auto-opening inventory at the
    // Docks. The accepted-live-object diagnostic captured a two-frame
    // auto-inventory GUI/live-object burst; pin the exact EE writer output so
    // the large combined-record path stays byte-for-byte owned.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_live_seq42_auto_inventory_gui_20260524_legacy.bin"
    )
    .to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/hg_live_seq42_auto_inventory_gui_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw live HG seq42 GUI stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(
        payload.as_slice(),
        expected_ee,
        "live HG seq42 rewrite should match the harness-dumped EE bytes"
    );
    assert!(
        claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
        "live HG seq42 should retain GUI live-object row ownership"
    );
    assert!(
        claim.records_examined > 1,
        "live HG seq42 should remain a combined live-object burst"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn hg_live_docks_091731_live_objects_match_dumped_ee_shapes() {
    // Live HG Docks run from 2026-05-24 with seeded nwsync, auto inventory,
    // dialog, and creature probes. The accepted-live-object diagnostic dumped
    // each bounded typed rewrite. Pin the exact EE writer bytes so these
    // different Docks object/update shapes cannot drift into broad acceptance.
    for (name, legacy, expected_ee, legacy_already_exact) in [
        (
            "seq28",
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq28_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq28_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq29",
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq29_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq29_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq34",
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq34_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq34_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq35",
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq35_exact_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq35_exact_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            true,
        ),
        (
            "seq42_auto_inventory_gui",
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq42_auto_inventory_gui_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/hg_live_docks_091731_seq42_auto_inventory_gui_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
    ] {
        let mut payload = legacy.to_vec();
        let raw_claim = super::claim_payload_if_verified(&payload);

        if legacy_already_exact {
            assert!(
                raw_claim.is_some(),
                "{name} should document a live HG packet already in exact EE live-object shape"
            );
        } else {
            assert!(
                raw_claim.is_none(),
                "{name} raw live HG stream should document the legacy Diamond shape"
            );
            let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
            assert!(
                claim.records_examined >= 1,
                "{name} should retain typed live-object records after rewrite"
            );
        }

        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the live HG EE byte shape"
        );
        let claim = super::claim_payload_if_verified(&payload)
            .expect("{name} final payload should exact-claim");
        if name == "seq42_auto_inventory_gui" {
            assert!(
                claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                "{name} should retain GUI live-object row ownership"
            );
            assert!(
                claim.records_examined > 1,
                "{name} should remain a combined live-object burst"
            );
        } else {
            assert!(
                claim.records_examined >= 1,
                "{name} should retain typed live-object ownership"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_prelude_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local Prelude harness run from 2026-05-24 reached gameplay with no root
    // quarantine. The accepted-live-object diagnostics captured both the raw
    // Diamond inputs and exact EE writer outputs for area entry plus the later
    // auto-inventory GUI stream.
    for (name, legacy, expected_ee, expect_gui) in [
        (
            "seq10_area_entry",
            include_bytes!(
                "../../../fixtures/live_object/local_prelude_seq10_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_prelude_seq10_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq19_auto_inventory_gui",
            include_bytes!(
                "../../../fixtures/live_object/local_prelude_seq19_auto_inventory_gui_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_prelude_seq19_auto_inventory_gui_20260524_ee.bin"
            )
            .as_slice(),
            true,
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw Prelude stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE byte shape"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain typed live-object records after rewrite"
        );
        if expect_gui {
            assert!(
                claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                "{name} should retain live GUI row ownership"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_contest_champions_area_entry_liveobject_matches_dumped_ee_shape() {
    // Local Contest Of Champions 0492 harness run from 2026-05-24 at area
    // entry. The accepted-live-object diagnostic dumped the raw Diamond
    // combined-record live-object payload and the exact EE rewrite; keep this
    // pinned as fixture evidence without widening the validator.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_contest_champions_seq11_area_entry_liveobject_20260524_legacy.bin"
    )
    .to_vec();
    let expected_ee = include_bytes!(
        "../../../fixtures/live_object/local_contest_champions_seq11_area_entry_liveobject_20260524_ee.bin"
    )
    .as_slice();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Contest Of Champions seq11 stream should document the legacy Diamond shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(
        payload.as_slice(),
        expected_ee,
        "Contest Of Champions seq11 rewrite should match the harness-dumped EE bytes"
    );
    assert!(
        claim.records_examined >= 1,
        "area-entry live-object payload should stay parsed as typed records"
    );
    assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_winds_eremor_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local The Winds of Eremor harness run from 2026-05-24. These diagnostics
    // captured both sides of three representative streams: the initial
    // placeable expansion, a larger placeable add/update burst, and the later
    // auto-inventory GUI body. Keep all three pinned to the exact EE writer
    // bytes instead of only proving that the final validator accepts them.
    for (name, legacy, expected_ee) in [
        (
            "initial_placeables",
            include_bytes!(
                "../../../fixtures/live_object/local_winds_eremor_seq_initial_placeables_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_winds_eremor_seq_initial_placeables_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "placeable_burst",
            include_bytes!(
                "../../../fixtures/live_object/local_winds_eremor_seq_placeable_burst_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_winds_eremor_seq_placeable_burst_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "auto_inventory_gui",
            include_bytes!(
                "../../../fixtures/live_object/local_winds_eremor_seq_auto_inventory_gui_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_winds_eremor_seq_auto_inventory_gui_20260524_ee.bin"
            )
            .as_slice(),
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw Winds of Eremor stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE byte shape"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain typed live-object records after rewrite"
        );
        if name == "auto_inventory_gui" {
            assert!(
                claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                "{name} should retain live GUI row ownership"
            );
        } else {
            assert!(
                claim.add_records + claim.update_records >= 1,
                "{name} should retain materialized add/update records"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_cepv22_current_player_effect_gui_stream_rewrites_to_exact_shape() {
    // Local CEP v2.2 builder harness capture from 2026-05-20 after the compact
    // area/live-object startup repairs. The stream starts with the same
    // decompile-owned compact `U/5 0x00000008` current-player status/effect
    // row family as Dark Ranger, then continues into GUI/update rows in one
    // exact live-object payload.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_cepv22_builder_seq26_current_player_effect_gui_20260520.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw CEP v2.2 current-player effect/GUI stream is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("CEP v2.2 current-player effect/GUI stream should rewrite");
    assert!(
        rewrite.update_records_rewritten >= 1
            || rewrite.bytes_inserted > 0
            || rewrite.interleaved_fragment_spans_promoted > 0,
        "CEP v2.2 stream should make typed rewrite progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten CEP v2.2 stream should validate exactly");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.live_gui_read_buffer_records >= 1);
}

#[test]
fn hg_starc5_seq37_creature_effect_delta_stream_rewrites_to_exact_shape() {
    // Live HG Starcore5 driver-only capture from 2026-05-18. The stream starts
    // with a standalone creature `U/5 0x00000008` looping visual-effect delta,
    // then continues with creature appearance/add/update records in the same
    // `GameObjUpdate_LiveObject` payload. EE's current reader consumes an
    // ObjectVisualTransformData map after each status-effect entry, so this
    // fixture must be owned by the focused effects/creature path and then
    // exact-claimed. It must not fall back to raw zlib or generic passthrough.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq37_creature_effect_delta_20260518.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured seq37 creature status-effect stream should rewrite");
    assert!(
        rewrite.bytes_inserted >= 32,
        "four status-effect entries should receive EE identity transform maps: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq37 creature status-effect stream should validate exactly");
    assert!(claim.update_records >= 1);
    assert!(claim.creature_appearance_records >= 1 || claim.creature_update_records >= 1);
}

#[test]
fn hg_starc5_seq37_otis_appearance_stream_rewrites_without_legacy_false_positive() {
    // Live HG Starcore5 driver-only capture from 2026-05-18 after the
    // decompile-backed full-appearance guard was added. This stream carries an
    // NPC full `P/5` appearance for "Otis"; the legacy body/equipment shape
    // must be widened into EE's 0x2001/0x23 dialect, not accepted as a shorter
    // already-EE record.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq37_otis_appearance_20260518.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured seq37 Otis appearance stream should rewrite");
    assert!(
        rewrite.bytes_inserted > 0 || rewrite.bits_inserted > 0,
        "Otis full appearance should require an explicit typed EE rewrite: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq37 Otis appearance stream should validate exactly");
    assert!(claim.creature_appearance_records >= 1);
    assert_no_full_creature_appearance_has_longer_legacy_shape(&payload);
}

#[test]
fn hg_starc5_seq36_town_npc_appearance_stream_rewrites_to_exact_shape() {
    // Live HG Starcore5 driver-only capture from 2026-05-18. This post-area
    // burst adds and updates several creature NPCs; the first unclaimed family
    // is a `P/5` creature appearance for "Town Greeter", followed by another
    // appearance/update pair for "Northern Trader". The appearance parser must
    // own the embedded visible-equipment subobjects and exact fragment cursor,
    // instead of letting the generic live-object scanner split on interior
    // `D`/`U`-looking bytes.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq36_town_greeter_trader_appearance_20260518.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured seq36 town NPC appearance stream should rewrite");
    assert!(
        rewrite.bytes_inserted > 0
            || rewrite.bits_inserted > 0
            || rewrite.update_records_rewritten > 0,
        "seq36 stream should make typed appearance/update progress: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq36 town NPC appearance stream should validate exactly");
    assert!(claim.creature_appearance_records >= 2);
    assert!(claim.creature_update_records >= 1);
    assert_no_full_creature_appearance_has_longer_legacy_shape(&payload);
}

#[test]
fn raw_legacy_prefixed_town_trader_appearance_is_not_claimed_as_ee_shape() {
    // Live HG Starcore5 driver-only capture from 2026-05-18. The Northern
    // Trader `P/5` full appearance has a Diamond body-table prefix whose second
    // byte is `0x13`. EE's widened reader can otherwise mistake that byte for
    // an already-EE full-body selector and stop before the counted visible
    // equipment list. Diamond's full appearance reader proves the longer
    // semantic record, so the strict EE byte-shape probe must reject the short
    // shifted candidate and force the typed LegacyDiamond -> EE rewrite.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq36_town_greeter_trader_appearance_20260518.bin"
    );
    let live = live_object_read_window(payload);
    let trader_offset = live
        .windows(2)
        .enumerate()
        .filter_map(|(offset, pair)| {
            (pair == [b'P', super::CREATURE_OBJECT_TYPE]).then_some(offset)
        })
        .last()
        .expect("fixture should contain the Northern Trader P/5 appearance");
    let legacy_end = super::appearance::try_get_legacy_creature_appearance_record_end(
        live,
        trader_offset,
        live.len(),
    )
    .expect("Diamond reader should prove the full trader appearance");
    let ee_end = super::appearance::try_get_ee_creature_appearance_record_end_by_byte_shape(
        live,
        trader_offset,
        live.len(),
    );

    assert!(
        ee_end.is_none_or(|ee_end| ee_end >= legacy_end),
        "short shifted EE byte-shape candidate must not hide longer Diamond P/5 appearance: ee_end={ee_end:?} legacy_end={legacy_end}"
    );
}

#[test]
fn hg_starc5_self_d5ff_gui_rows_seq49_rewrites_and_claims_exactly() {
    // Starcore5 driver-only 2026-05-13 capture, quarantined as
    // `GameObjUpdate_LiveObject` seq49. The stream begins with a self `U/5`
    // C408 status update, then a deterministic self `I/0xD5FF` inventory
    // record, then GUI inventory rows (`G I A`) in the same high-level
    // live-object payload. This proves the D5FF inventory read cursor and any
    // adjacent fragment storage are handled by the inventory/span translators
    // before the GUI row family is allowed to claim its nested item records.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_self_d5ff_gui_rows_seq49_live_20260513_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload).expect(
        "captured self D5FF + GUI-row stream should rewrite through typed live-object path",
    );
    assert!(
        rewrite.update_records_examined > 0
            || rewrite.interleaved_fragment_spans_promoted > 0
            || rewrite.bytes_inserted > 0
            || rewrite.bytes_removed > 0,
        "D5FF GUI-row stream should make typed rewrite progress: {rewrite:?}"
    );

    let _claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten self D5FF + GUI-row stream should be exactly claimable");
}

#[test]
fn hg_starc5_area_entry_d5ff_creature_inventory_terminal_tail_rewrites_to_exact_claim() {
    // Starcore5 driver-only 2026-05-17 area-entry capture, quarantined after
    // the live-object rewrite inserted EE creature-add visual-transform bytes
    // but before the exact validator could claim the self creature inventory
    // `I/0xD5FF` record. This fixture keeps the fix packet-family owned:
    // Diamond `sub_455940` and EE `sub_1407B4F70` both drive the inventory
    // mask branches, so the record must be parsed as typed inventory rather
    // than relaxed as raw zlib or a generic live-object blob. The terminal
    // fragment tail is transport storage, not a D5FF reader branch: raw exact
    // claim must reject it, then the rewrite pass trims it and proves the final
    // EE-shaped payload.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_area_entry_d5ff_creature_inventory_20260517_rejected.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw terminal D5FF storage must not be accepted as reader-owned bits"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("typed D5FF terminal storage should be removed by the rewrite pass");
    assert!(
        rewrite.fragment_bits_trimmed > 0,
        "D5FF terminal storage should be trimmed after typed cursor proof: {rewrite:?}"
    );

    let _claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten area-entry D5FF creature inventory stream should claim exactly");
}

#[test]
fn captured_hg_sooty_transition_c40f_self_status_stream_rewrites_to_exact_shape() {
    // Starcore5 driver-only Sooty Crow transition capture, quarantined as
    // `GameObjUpdate_LiveObject` seq41 on 2026-05-12. The first record is a
    // `U/5` self-creature update with mask `0x0000_C40F`: Diamond writes the
    // lower position/orientation/action branches first, then the same
    // self/status suffix used by `0xC408`, followed by a three-byte adjacent
    // CNW fragment-storage span before the next inventory record. This fixture
    // proves the packet is not raw-passed: the live-object translator must
    // promote that span and then the exact EE-shape claim must own the result.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_transition_seq41_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("captured HG C40F self/status stream should rewrite");
    assert!(rewrite.interleaved_fragment_spans_promoted >= 1);
    assert!(rewrite.interleaved_fragment_bytes_promoted >= 3);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten HG C40F self/status stream should be exactly claimable");
    assert!(claim.creature_update_records >= 1);
    assert!(claim.inventory_records >= 1);
}

#[test]
fn captured_hg_transition_seq48_delete_inventory_door_stream_is_claimed() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_transition_seq48_delete_inventory_door.bin"
    )
    .to_vec();
    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.delete_records >= 1);
    assert!(claim.inventory_records >= 1);
    assert!(claim.add_records >= 1);
    assert!(claim.update_records >= 1);
}

#[test]
fn captured_hg_starc5_seq48_door_sign_transition_stream_is_claimed() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_seq48_door_sign_transition_unclaimed.bin"
    )
    .to_vec();

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 7);
    assert!(claim.update_records >= 7);
}

#[test]
fn hg_inventory_2e00_followed_by_gq_rows_claims_exactly() {
    let payload =
        include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin");
    let claim = super::claim_payload_if_verified(payload)
        .expect("HG I/0x2E00 inventory plus GQ row stream should be exactly claimed");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 22);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.creature_update_records, 3);
    assert_eq!(claim.delete_records, 1);
    assert_eq!(claim.records_examined, 6);
}

#[test]
fn hg_inventory_2e00_gui_hand_trap_sentinel_claims_exactly() {
    let mut payload =
        include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin")
            .to_vec();
    let inventory_offset = 7usize;
    assert_eq!(payload[inventory_offset], b'I');
    payload[inventory_offset + 1..inventory_offset + 5]
        .copy_from_slice(&0xFFFF_FFECu32.to_le_bytes());

    let claim = super::claim_payload_if_verified(&payload)
        .expect("HG I/0x2E00 inventory with 0xFFFFFFEC GUI sentinel should be exactly claimed");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 22);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.records_examined, 6);
}

#[test]
fn hg_inventory_2e00_gui_hand_trap_false_0800_tail_claims_exactly() {
    let mut payload =
        include_bytes!("../../fixtures/live_object/hg_inventory_2e00_gq_rows_u_updates.bin")
            .to_vec();
    let inventory_offset = 7usize;
    assert_eq!(payload[inventory_offset], b'I');
    payload[inventory_offset + 1..inventory_offset + 5]
        .copy_from_slice(&0xFFFF_FFECu32.to_le_bytes());

    let declared = usize::try_from(super::read_u32_le(&payload, 3).unwrap()).unwrap();
    let mut fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();
    let branch_0800_bit = super::CNW_FRAGMENT_HEADER_BITS + 1 + 2 + (6 * 3);
    fragment_bits[branch_0800_bit] = false;
    let repacked = super::bits::pack_msb_valid_bits(fragment_bits, super::CNW_FRAGMENT_HEADER_BITS);
    payload.truncate(declared);
    payload.extend_from_slice(&repacked);

    let claim = super::claim_payload_if_verified(&payload).expect(
        "HG I/0x2E00 0xFFFFFFEC inventory with false 0x0800 interleaved tail should claim exactly",
    );

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 22);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.records_examined, 6);
}

#[test]
fn hg_inventory_2e00_gui_hand_trap_interleaved_tail_promotes_before_gq() {
    // Live HG 2026-05-17 exposed this precise area-entry sibling:
    //
    //   I FFFFFFEC mask=0x2E00
    //     0x0400 clear/set one slot
    //     0x0200 false second branch + DWORD zero
    //     0x2000 second object-list count five
    //     0x0800 false
    //     twelve bytes of adjacent CNW fragment storage
    //   G Q ...
    //
    // Diamond `sub_455940` and EE `sub_1407B4F70` both read the 0x0800
    // fixed 12-byte branch only when its BOOL is true. Therefore the false
    // branch owns no read bytes; the twelve bytes immediately before `GQ`
    // must be promoted to the fragment bitstream before strict validation.
    let mut live = Vec::new();
    live.extend_from_slice(&[
        b'I', 0xEC, 0xFF, 0xFF, 0xFF, 0x00, 0x2E, 0x01, 0x6B, 0x01, 0x6B, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xEC, 0xFF, 0xFF, 0xFF, 0xF4, 0x71, 0x01,
        0x80, 0xE7, 0x71, 0x01, 0x80, 0xE2, 0x71, 0x01, 0x80, 0xFB, 0x71, 0x01, 0x80,
    ]);
    live.extend_from_slice(&[
        0x0E, 0x16, 0x14, 0x12, 0x40, 0x0E, 0x1E, 0x26, 0x24, 0x22, 0x50, 0x1E,
    ]);
    live.extend_from_slice(&[b'G', b'Q', 0x00]);

    let declared = super::HIGH_LEVEL_HEADER_BYTES + super::CNW_LENGTH_BYTES + live.len();
    let mut payload = vec![
        b'P',
        super::GAME_OBJECT_UPDATE_MAJOR,
        super::LIVE_OBJECT_MINOR,
    ];
    payload.extend_from_slice(&(declared as u32).to_le_bytes());
    payload.extend_from_slice(&live);
    payload.extend_from_slice(&super::bits::pack_msb_valid_bits(
        vec![false; super::CNW_FRAGMENT_HEADER_BITS],
        super::CNW_FRAGMENT_HEADER_BITS,
    ));

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw interleaved fragment storage must not claim before promotion"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("inventory-owned interleaved fragment storage should be promoted");
    assert_eq!(rewrite.records_examined, 2);
    assert_eq!(rewrite.interleaved_fragment_spans_promoted, 1);
    assert_eq!(rewrite.interleaved_fragment_bytes_promoted, 12);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("promoted hand-trap inventory plus GQ stream should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 19);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.records_examined, 2);
}

#[test]
fn hg_inventory_2e01_followed_by_gq_rows_splits_on_decompiled_cursor() {
    let payload = include_bytes!(
        "../../fixtures/live_object/hg_starc5_seq42_mixed_door_placeable_unclaimed_len519.bin"
    );
    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let live = &payload[7..declared];
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();

    let inventory_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        live,
        0,
        live.len(),
    );
    assert_eq!(
        inventory_end, 56,
        "0x2E01 should consume the compact 0x0001 branch, then 0x0400/0x0200/0x2000/0x0800, and stop at GQ"
    );

    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    let inventory_claim = super::inventory::advance_verified_inventory_record(
        live,
        0,
        inventory_end,
        &fragment_bits,
        &mut bit_cursor,
    )
    .expect("0x2E01 inventory prefix should be exactly claimed before GQ");
    assert_eq!(inventory_claim.fragment_bits, 13);
    assert_eq!(bit_cursor, super::CNW_FRAGMENT_HEADER_BITS + 13);

    let gui_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        live,
        inventory_end,
        live.len(),
    );
    assert_eq!(gui_end, 221);
    let gui_claim = super::gui::advance_verified_live_gui_record(
        live,
        inventory_end,
        gui_end,
        &fragment_bits,
        &mut bit_cursor,
    )
    .expect("GQ quickbar-link row stream should be exactly claimed after inventory");
    assert_eq!(gui_claim.fragment_bits, 0);

    let mut rewritten = payload.to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("captured Starcore5 stream should rewrite legacy A/5 creature adds before claim");
    assert!(
        rewrite.bytes_inserted >= 8,
        "legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("captured Starcore5 0x2E01 inventory/GQ live-object stream should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 13);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
}

#[test]
fn hg_docks_inventory_2e01_missing_feature25_second_count_rewrites_before_claim() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_docks_seq36_inventory_2e01_missing_feature25_second_count_len454.bin"
    );
    assert!(
        super::claim_payload_if_verified(payload).is_none(),
        "raw legacy I/0x2E01 missing Feature-25 second_count must not claim as EE-shaped"
    );

    let declared = usize::try_from(super::read_u32_le(payload, 3).unwrap()).unwrap();
    let live = &payload[7..declared];
    let fragment_bits =
        super::bits::decode_msb_valid_bits(&payload[declared..], super::CNW_FRAGMENT_HEADER_BITS)
            .unwrap();

    let inventory_end = super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
        live,
        0,
        live.len(),
    );
    assert_eq!(
        inventory_end, 66,
        "raw HG I/0x2E01 quickbar-link record should split before the following GQ rows"
    );

    let mut bit_cursor = super::CNW_FRAGMENT_HEADER_BITS;
    let inventory_claim = super::inventory::advance_verified_inventory_record(
        live,
        0,
        inventory_end,
        &fragment_bits,
        &mut bit_cursor,
    )
    .expect("captured I/0x2E01 quickbar-link inventory record should claim exactly");
    assert_eq!(inventory_claim.fragment_bits, 20);
    assert_eq!(bit_cursor, super::CNW_FRAGMENT_HEADER_BITS + 20);

    let mut rewritten = payload.to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("captured Docks live-object stream should rewrite before strict claim");
    assert!(
        rewrite.bytes_inserted >= 8,
        "the following legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("rewritten Docks live-object stream should claim with exact family validators");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 20);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn hg_sooty_crow_npc_creature_span_rewrites_and_claims_exactly() {
    let payload =
        include_bytes!("../../../fixtures/live_object/hg_sooty_crow_npc_creature_span_len495.bin");
    let mut rewritten = payload.to_vec();
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("Sooty Crow NPC creature live-object span should have a focused rewrite");

    assert!(rewrite.interleaved_fragment_spans_promoted >= 1);
    assert!(rewrite.interleaved_fragment_bytes_promoted >= 3);
    assert!(
        rewrite.bytes_inserted >= 8,
        "the real legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("Sooty Crow NPC creature live-object span should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.creature_update_records, 3);
    assert_eq!(claim.creature_appearance_records, 2);
    assert_eq!(claim.add_records, 1);
}

#[test]
fn hg_sooty_crow_inventory_mask_2e01_extended_span_rewrites_and_claims_exactly() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_sooty_crow_inventory_mask_2e01_span_len611.bin"
    );
    let mut rewritten = payload.to_vec();
    let repair = crate::translate::live_object::declared_length_repair_candidates(&rewritten)
        .into_iter()
        .find(|candidate| usize::try_from(candidate.new_declared).ok() == Some(604))
        .expect(
            "Sooty Crow live-object span should prove the final CNW fragment tail at offset 604",
        );
    rewritten[3..7].copy_from_slice(&repair.new_declared.to_le_bytes());

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut rewritten)
        .expect("Sooty Crow I/0x2E01 extended live-object span should have a focused rewrite");

    assert!(
        rewrite.bytes_inserted >= 8,
        "the following legacy A/5 creature add should receive EE visual-transform storage"
    );

    let claim = super::claim_payload_if_verified(&rewritten)
        .expect("Sooty Crow I/0x2E01 extended live-object span should claim exactly after rewrite");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 2);
}

#[test]
fn hg_starc5_sooty_current_seq35_u5_visual_transform_update_rewrites_and_claims_exactly() {
    // Starcore5 driver-only Sooty Crow capture from 2026-05-12. The server sends
    // a stale `P 05 01` declared length (`0x5D`) that overruns the decompile-
    // proven legacy A/5 creature-add read cursor by six CNW fragment-storage
    // bytes. This fixture must go through the declared-length repair search,
    // then the focused live-object record translators, then the exact EE-shape
    // claim. It is intentionally not raw-passthrough.
    let payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_current_seq35_u5_inventory_20260512_unclaimed.bin"
    );

    let mut accepted = None;
    for repair in crate::translate::live_object::declared_length_repair_candidates(payload) {
        let mut candidate = payload.to_vec();
        candidate[3..7].copy_from_slice(&repair.new_declared.to_le_bytes());

        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);
        let _ =
            crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut candidate,
                None,
            );
        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);
        let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut candidate);
        let _ =
            crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
                &mut candidate,
                None,
            );
        let _ = super::rewrite_update_records_payload_if_possible(&mut candidate);

        if let Some(claim) = super::claim_payload_if_verified(&candidate) {
            accepted = Some((repair, claim));
            break;
        }
    }

    let (repair, claim) = accepted.expect(
        "current Starcore5 Sooty Crow U/5 visual-transform span should repair and claim exactly",
    );
    assert_ne!(repair.new_declared, repair.old_declared);
    // The quarantined capture was initially named from the visible inventory
    // symptom, but the decompile-backed live-object opcode is `U` for object
    // type 5 with mask 0x47. The following `I/FE` bytes are chunk-local CNW
    // fragment storage, not an inventory row and not the one-byte legacy
    // visual-transform selector.
    assert_eq!(claim.inventory_records, 0);
    assert_eq!(claim.creature_visual_transform_update_records, 0);
    assert!(claim.creature_update_records >= 1);
    assert!(claim.add_records >= 1);
}

#[test]
fn hg_starc5_seq54_coalesced_liveobject_burst_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_seq54_coalesced_liveobject_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("Starcore5 coalesced live-object burst should have an exact semantic claim");

    assert_eq!(claim.inventory_records, 1);
    assert!(claim.add_records >= 1);
    assert!(claim.creature_appearance_records >= 1);
    assert!(claim.creature_update_records >= 2);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq53_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq53_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("Starcore5 GUI inventory item-create stream should have a focused rewrite");
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten Starcore5 GUI inventory item-create stream should claim exactly");
    assert!(claim.live_gui_item_create_records > 0);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq53_live_20260512_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq53_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("live Starcore5 GUI inventory item-create stream should have a focused rewrite");
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten live Starcore5 GUI inventory item-create stream should claim exactly");
    assert!(claim.live_gui_item_create_records > 0);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq54_live_20260512_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq54_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload).expect(
        "second live Starcore5 GUI inventory item-create stream should have a focused rewrite",
    );
    assert!(
        rewrite.bits_inserted > 0 || rewrite.bytes_inserted > 0,
        "legacy GUI item-create rows should receive EE item-create extras"
    );

    let claim = super::claim_payload_if_verified(&payload).expect(
        "rewritten second live Starcore5 GUI inventory item-create stream should claim exactly",
    );
    assert!(claim.live_gui_item_create_records > 0);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_starc5_gui_inventory_gia_seq54_live_20260512_survives_dispatch_rewrite_sequence() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_gui_inventory_gia_seq54_live_20260512_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("dispatch rewrite sequence should preserve exact GUI inventory claim");
    assert!(claim.live_gui_item_create_records > 0);
}

#[test]
fn hg_starc5_sooty_transition_seq34_creature_update_c44f_claims_exactly() {
    // Starcore5 driver-only Sooty Crow transition capture from 2026-05-13.
    // This was quarantined as `GameObjUpdate_LiveObject` seq34 after the area
    // transition was otherwise stable. The first record is a legacy `U/5`
    // creature update with mask `0x0000_C44F`: the already verified `0xC40F`
    // self/status family plus the low `0x0040` creature state branch.
    //
    // EE `CNWSMessage::WriteGameObjUpdate_UpdateObject` keeps `0x0040` as a
    // typed creature state branch: optional pre-state BOOL, then WORD/BYTE/
    // WORD/BYTE/BOOL with an optional target OBJECTID when mode is 2. The proxy
    // must prove that full cursor shape instead of treating the deflated live
    // object payload as a raw zlib blob or generic passthrough.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_sooty_transition_seq34_creature_update_c44f_20260513_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("Sooty transition U/5 0xC44F creature stream should claim exactly");
    assert!(claim.creature_update_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn local_diamond_seq15_coalesced_liveobject_burst_rewrites_and_claims_exactly() {
    // Local Diamond bridge capture from 2026-05-16, quarantined from coalesced
    // server sequence 15 as an unclaimed `GameObjUpdate_LiveObject` payload.
    // The first live-object record is a legacy `A/5` creature add followed by
    // creature appearance/update records.  This fixture keeps the coalesced
    // path honest: transport repair alone must not own it, and the payload may
    // be emitted only after the typed live-object add/update translators make
    // the stream match exact EE reader shape.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("local Diamond seq15 coalesced live-object burst should claim exactly");

    assert!(claim.add_records >= 1);
    assert!(claim.creature_appearance_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn local_diamond_seq15_compact_creature_ids_canonicalize_to_ee_external_namespace() {
    // EE creature add (`sub_14077F870`) calls
    // `CGameObjectArray::AddExternalObject(&id, creature, ...)`, which stores
    // the object in the external high-bit namespace. EE creature appearance
    // (`sub_14077FE10`) then resolves the appearance `OBJECTID` through that
    // object array before consuming the body. The local Diamond seq15 capture
    // used compact creature id `0x000000FE` for both `A/5` and later `P/5`
    // records; leaving that compact id in the EE-facing stream reproduces the
    // client log warning `HandleServerToPlayerCreatureUpdate_Appearance:
    // EXOWARNING: pCreature`.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let before = super::claim_payload_if_verified(&payload)
        .expect("rewritten seq15 creature burst should be exact before id canonicalization");
    assert!(before.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x0000_00FE
    }));
    assert!(before.mentions.iter().any(|mention| {
        mention.opcode == b'P'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x0000_00FE
    }));

    let summary = super::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
        .expect("compact creature add/appearance ids should canonicalize for EE");
    assert_eq!(summary.compact_add_ids_observed, 1);
    assert_eq!(summary.add_ids_rewritten, 1);
    assert!(
        summary.reference_ids_rewritten >= 1,
        "at least the P/5 appearance reference must be rewritten"
    );

    let after = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("canonicalized seq15 creature burst should remain exact and lifecycle-safe");
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x8000_00FE
    }));
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'P'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x8000_00FE
    }));
    assert!(!after.mentions.iter().any(|mention| {
        matches!(mention.opcode, b'A' | b'P' | b'U' | b'D')
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x0000_00FE
    }));
}

#[test]
fn local_diamond_seq15_creature_ids_can_use_playerlist_proven_session_alias() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq15_coalesced_liveobject_20260516_unclaimed.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    super::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
        .expect("generic compact canonicalization documents the pre-session state");

    let summary = super::canonicalize_player_session_creature_ids_payload_for_ee(
        &mut payload,
        |compact_id| (compact_id == 0xFE).then_some(0xFFFF_FFFE),
    )
    .expect("PlayerList-proven compact creature alias should rewrite the add/appearance pair");
    assert_eq!(summary.compact_add_ids_observed, 1);
    assert_eq!(summary.add_ids_rewritten, 1);
    assert!(
        summary.reference_ids_rewritten >= 1,
        "at least the P/5 appearance reference must be rewritten to the session id"
    );

    let after = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("session-canonicalized seq15 creature burst should remain exact");
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0xFFFF_FFFE
    }));
    assert!(after.mentions.iter().any(|mention| {
        mention.opcode == b'P'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0xFFFF_FFFE
    }));
    assert!(!after.mentions.iter().any(|mention| {
        matches!(mention.opcode, b'A' | b'P' | b'U' | b'D')
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == 0x8000_00FE
    }));
}

#[test]
fn local_diamond_seq17_inventory_2a00_requires_exact_branch_bits() {
    // Local Diamond bridge capture from 2026-05-17 after the preceding
    // Diamond-only missing-object `U/5` update was removed. The remaining
    // `I/0x2A00` record is not raw passthrough: Diamond `sub_455940` and EE
    // `sub_1407B4F70` both read it as `0x0200 | 0x2000 | 0x0800`, with BOOL
    // branch bits choosing the read-buffer layout. The exact claim must
    // therefore prove those branch bits before the following `GQ` record can
    // be treated as the next live-object submessage.
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq17_inventory_2a00_20260517_rewritten.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("local Diamond seq17 I/0x2A00 + GQ stream should claim exactly");
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);

    let fragment_offset = u32::from_le_bytes(payload[3..7].try_into().unwrap()) as usize;
    assert!(fragment_offset + 1 < payload.len());

    let mut wrong_0200_branch = payload.to_vec();
    wrong_0200_branch[fragment_offset] &= !0x08;
    assert!(
        super::claim_payload_if_verified(&wrong_0200_branch).is_none(),
        "0x0200 byte-mask shape must require its second BOOL branch bit"
    );

    let mut wrong_0800_branch = payload.to_vec();
    wrong_0800_branch[fragment_offset + 1] &= !0x80;
    assert!(
        super::claim_payload_if_verified(&wrong_0800_branch).is_none(),
        "0x0800 12-byte tail shape must require its present BOOL branch bit"
    );
}

#[test]
fn local_diamond_seq17_sentinel_inventory_owner_is_removed_with_missing_update() {
    // Same local Diamond 2026-05-17 direct M payload before lifecycle cleanup.
    // EE's inventory reader checks the resolved object pointer before consuming
    // the `0x2000` Feature-25 body, so the sentinel-owner `I/0x2A00` record
    // must be removed together with the preceding missing-object `U/5` record.
    // Leaving it in place reproduces EE's `Unknown Update sub-message` because
    // the reader bails before the following `GQ` bytes are consumed.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq17_liveobject_sentinel_inventory_20260517.bin"
    )
    .to_vec();

    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let rewrite =
        super::remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, _| false)
            .expect("sentinel U/I records should be removed after exact lifecycle proof");
    assert_eq!(rewrite.removed_update_records, 2);

    let claim = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("remaining GQ live-object record should claim after sentinel removals");
    assert_eq!(claim.inventory_records, 0);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_seq34_zero_mask_creature_updates_rewrite_and_lifecycle_clean() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_seq34_zero_mask_creature_updates_20260522.bin"
    )
    .to_vec();
    let live = live_object_read_window(&payload);
    assert!(
        super::inventory::try_get_legacy_live_inventory_fragment_bit_count(live, 15, 28).is_some(),
        "I/0x0100 D-entry inventory row should prove the following U/5 boundary"
    );
    assert_eq!(
        super::boundary::find_next_legacy_live_object_sub_message_boundary_after(
            live,
            15,
            live.len(),
        ),
        28
    );

    let pre_canonical = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert_eq!(pre_canonical.inventory_records, 1);
    assert_eq!(pre_canonical.creature_visual_transform_update_records, 1);
    assert!(
        pre_canonical.creature_update_records >= 1,
        "captured XP2 stream should carry exact zero-mask creature no-op updates before cleanup"
    );

    let session_creature_id = 0xFFFF_FFFE;
    let canonical = super::canonicalize_player_session_creature_ids_payload_for_ee(
        &mut payload,
        |compact_id| (compact_id == 0xFE).then_some(session_creature_id),
    )
    .expect("PlayerList-proven compact creature owner should canonicalize for EE");
    assert!(
        canonical.reference_ids_rewritten >= 2,
        "the visual-transform U/5 and inventory owner should both use the session id"
    );

    let cleanup =
        super::remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, id| {
            id == session_creature_id
        })
        .expect("zero-mask missing creature updates should be removable after exact proof");
    assert!(cleanup.diamond_missing_object_update_records >= 1);

    let claim = super::claim_payload_if_verified_with_lifecycle(&payload, |_, id| {
        id == session_creature_id
    })
    .expect("XP2 zero-mask creature burst should be lifecycle-safe after cleanup");
    assert!(
        claim
            .mentions
            .iter()
            .any(|mention| { mention.opcode == b'I' && mention.object_id == session_creature_id })
    );
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == super::CREATURE_OBJECT_TYPE
            && mention.object_id == session_creature_id
    }));
    assert_eq!(claim.creature_update_records, 0);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_seq26_current_player_d5ff_inventory_terminal_tail_rewrites_to_exact_claim() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_seq26_lifecycle_unverified_combined_20260522.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw current-player D5FF terminal storage must not be reader-owned"
    );
    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("current-player D5FF terminal storage should rewrite after typed proof");
    assert!(
        rewrite.fragment_bits_trimmed > 0,
        "current-player D5FF terminal storage should be trimmed: {rewrite:?}"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten current-player D5FF inventory should claim exactly");
    assert_eq!(claim.add_records, 1);
    assert_eq!(claim.creature_appearance_records, 1);
    assert_eq!(claim.creature_update_records, 1);
    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
    assert!(
        claim
            .mentions
            .iter()
            .any(|mention| mention.opcode == b'I' && mention.object_id == 0xFFFF_FFFE)
    );

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("preceding A/5 materializes current player before D5FF inventory");
    assert_eq!(lifecycle_claim.inventory_records, 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_seq19_door_placeable_gui_stream_rewrites_to_exact_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_seq19_door_placeable_gui_stream_20260522_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw XP2 seq19 stream documents the unclaimed Diamond door/placeable + GUI-row shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 1);
    assert!(claim.update_records >= 1);
    assert!(
        claim.live_gui_item_create_records >= 1,
        "fixture should retain the GUI-row item-create suffix"
    );
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter1_seq19_placeable_door_stream_rewrites_to_exact_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_chapter1_seq19_placeable_door_stream_20260523_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Chapter1 stream documents the unclaimed Diamond door/placeable shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 1);
    assert!(claim.update_records >= 1);
    assert!(
        claim.mentions.iter().any(|mention| {
            mention.opcode == b'A'
                && matches!(
                    mention.object_type,
                    super::PLACEABLE_OBJECT_TYPE | super::DOOR_OBJECT_TYPE
                )
        }),
        "rewritten Chapter1 stream should retain materializing door/placeable adds"
    );
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter1_seq19_transition_pending_chunk_waits_for_continuation() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_chapter1_seq19_transition_pending_chunk1_20260523_unclaimed.bin"
    );

    assert!(
        super::claim_payload_if_verified(payload).is_none(),
        "first Chapter1 transition pending chunk must not claim before its continuation provides the owned fragment tail"
    );
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter1_seq19_transition_pending_claimed_stream_stays_exact() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_chapter1_seq19_transition_pending_chunks2_20260523_claimed.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("two-chunk Chapter1 transition pending stream should exact-claim after rewrite");
    assert!(claim.add_records >= 1);
    assert!(claim.update_records >= 1);
    assert!(
        claim.mentions.iter().any(|mention| {
            mention.opcode == b'A'
                && matches!(
                    mention.object_type,
                    super::PLACEABLE_OBJECT_TYPE | super::DOOR_OBJECT_TYPE
                )
        }),
        "claimed Chapter1 transition stream should retain materializing door/placeable adds"
    );
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter1_seq20_transition_placeable_stream_rewrites_to_exact_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_chapter1_seq20_transition_placeable_stream_20260523_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Chapter1 transition stream documents the unclaimed Diamond placeable shape"
    );

    let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
    assert!(claim.add_records >= 1);
    assert!(claim.update_records >= 1);
    assert!(
        claim.mentions.iter().any(|mention| {
            mention.opcode == b'U' && mention.object_type == super::PLACEABLE_OBJECT_TYPE
        }),
        "rewritten Chapter1 transition stream should retain placeable updates"
    );
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter2e_area_entry_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local Diamond Chapter2E area-entry harness run from 2026-05-24. The
    // accepted-live-object diagnostic dumped both sides of each bounded rewrite,
    // so these fixtures pin the byte-exact EE writer shape rather than only the
    // final validator result.
    for (name, legacy, expected_ee) in [
        (
            "seq16",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq16_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq16_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq17",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq17_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq17_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq18",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq18_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq18_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq19",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq19_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter2e_seq19_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw Chapter2E stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite must match the harness-dumped EE byte shape",
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain at least one typed live-object record"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_chapter4_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local Diamond Chapter4 area-entry and auto-inventory harness run from
    // 2026-05-24. The accepted-live-object diagnostic dumped both the legacy
    // Diamond input and the bounded typed EE writer output; keep the exact
    // bytes pinned here rather than only checking final validator acceptance.
    for (name, legacy, expected_ee, expect_gui) in [
        (
            "seq12_area_entry",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter4_seq12_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter4_seq12_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq13_area_entry",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter4_seq13_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter4_seq13_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq23_auto_inventory",
            include_bytes!(
                "../../../fixtures/live_object/local_chapter4_seq23_auto_inventory_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_chapter4_seq23_auto_inventory_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            true,
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw Chapter4 stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE bytes"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain at least one typed live-object record"
        );
        if expect_gui {
            assert!(
                claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                "{name} should retain GUI live-object row ownership"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_chapter2_inventory_live_objects_rewrite_to_exact_shape() {
    for (name, fixture) in [
        (
            "seq13",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter2_seq13_liveobject_20260522_unclaimed.bin"
            )
            .as_slice(),
        ),
        (
            "seq14",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter2_seq14_liveobject_20260522_unclaimed.bin"
            )
            .as_slice(),
        ),
        (
            "seq16",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter2_seq16_liveobject_20260522_unclaimed.bin"
            )
            .as_slice(),
        ),
        (
            "seq38",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter2_seq38_inventory_creature_stream_20260522_unclaimed.bin"
            )
            .as_slice(),
        ),
    ] {
        let mut payload = fixture.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw XP2 Chapter 2 inventory stream should document the unclaimed Diamond shape"
        );

        assert!(
            crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
                &mut payload,
                None
            ),
            "{name} should rewrite through the bounded exact adapter"
        );
        let claim = super::claim_payload_if_verified(&payload).unwrap_or_else(|| {
            panic!("{name} rewritten live-object payload should exact-claim")
        });
        assert!(
            claim.add_records >= 1,
            "{name} should retain at least one materializing live-object add"
        );
        if name == "seq38" {
            assert_eq!(
                claim.inventory_records, 1,
                "seq38 should retain the compact current-player I/0x2000 Feature-25 row"
            );
            assert!(
                claim.creature_appearance_records >= 1,
                "seq38 should retain the following creature appearance record"
            );
        }
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_chapter1_area_entry_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local XP2_Chapter1 `xp2_intro` area-entry capture from 2026-05-24.
    // This module produced a dense sequence of deflated GameObjUpdate_LiveObject
    // frames while the EE client crossed the area-load gate. Keep each stream
    // pinned to the bounded typed live-object rewrite path and the exact
    // accepted-live-object EE bytes dumped by the harness.
    for (name, legacy, expected_ee) in [
        (
            "seq11",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq11_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq11_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq12",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq12_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq12_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq13",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq13_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq13_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq14",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq14_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq14_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq15",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq15_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq15_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq16",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq16_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq16_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq17",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq17_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq17_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq18",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq18_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq18_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq19",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq19_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq19_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq20",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq20_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq20_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq21",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq21_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq21_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq22",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq22_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq22_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
        (
            "seq23",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq23_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter1_seq23_liveobject_20260524_ee.bin"
            )
            .as_slice(),
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw XP2_Chapter1 stream should document the legacy Diamond shape"
        );

        let started = std::time::Instant::now();
        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "{name} XP2_Chapter1 live-object rewrite must stay bounded"
        );
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite must match the harness-dumped EE byte shape"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain at least one typed live-object record"
        );
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[test]
fn local_xp2_chapter2_u5_8008_effect_visibility_rewrites_to_exact_shape() {
    // Local Diamond XP2 Chapter 2 strict capture from 2026-05-22. The packet is
    // a single creature `U/5 0x00008008`: two compact visual-effect rows in the
    // read buffer and the 0x8000 visibility booleans in the CNW fragment byte.
    // EE build 8193 reads ObjectVisualTransformData after each effect row, so
    // the focused creature update translator must insert exactly those maps
    // while leaving the visibility bits in fragment storage.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_chapter2_seq38_u5_8008_effect_visibility_20260522_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw local XP2 0x8008 status/visibility update is still legacy-shaped"
    );

    let rewrite = super::rewrite_update_records_payload_if_possible(&mut payload)
        .expect("local XP2 0x8008 status/visibility update should rewrite");
    assert_eq!(
        rewrite.bytes_inserted, 16,
        "two visual-effect rows should receive EE identity transform maps"
    );
    assert!(
        rewrite.update_records_rewritten >= 1,
        "0x8008 stream should be owned by the typed creature update path"
    );

    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local XP2 0x8008 stream should validate exactly");
    assert_eq!(claim.creature_update_records, 1);
    assert_eq!(claim.fragment_bytes, 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_chapter3_gatesofcania_live_object_stream_rewrites_to_exact_shape() {
    // Local Diamond XP2 Chapter 3 gatesofcania startup strict capture from
    // 2026-05-23. The stream starts with a compact U/5 row and then alternates
    // placeable U/A records; every mutation must be owned by the focused
    // live-object rewriter before the exact EE reader validator accepts it.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_chapter3_seq13_live_object_20260523_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw local XP2 Chapter3 live-object stream should document the unclaimed Diamond shape"
    );

    assert!(
        crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
            &mut payload,
            None
        ),
        "local XP2 Chapter3 live-object stream should rewrite through the bounded exact adapter"
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local XP2 Chapter3 live-object stream should validate exactly");
    assert!(
        claim.update_records >= 6,
        "fixture should retain the U/5 and U/9 update records after exact rewrite"
    );
    assert!(claim.add_records >= 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_chapter3_inventory_gui_stream_rewrites_to_exact_shape() {
    // Same local XP2 Chapter 3 run after quickbar release. This post-inventory
    // stream contains compact GUI inventory appearance rows (`GIA`/`GRA`) and
    // must be owned by the typed live-object item adapter before strict release.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_chapter3_seq22_live_object_20260523_unclaimed.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw local XP2 Chapter3 inventory GUI stream should document the unclaimed Diamond shape"
    );

    assert!(
        crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
            &mut payload,
            None
        ),
        "local XP2 Chapter3 inventory GUI stream should rewrite through the bounded exact adapter"
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten local XP2 Chapter3 inventory GUI stream should validate exactly");
    assert!(
        claim.live_gui_item_create_records >= 8,
        "fixture should retain the five inventory and three repository GUI item-create rows"
    );
    assert!(
        claim.materialized_item_object_ids.len() >= 8,
        "GUI item-create rows should materialize the compact inventory item ids"
    );
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_chapter3_live_objects_rewrite_to_dumped_exact_ee_shape() {
    // Local Diamond XP2 Chapter 3 run from 2026-05-24 reached Gates of Cania
    // gameplay and auto-opened inventory. The accepted-live-object diagnostic
    // dumped both sides of the bounded rewrite, so pin the exact writer bytes
    // instead of only proving the final validator accepts the payload.
    for (name, legacy, expected_ee, expect_gui) in [
        (
            "seq12_area_entry",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter3_seq12_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter3_seq12_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq13_area_entry",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter3_seq13_area_entry_liveobject_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter3_seq13_area_entry_liveobject_20260524_ee.bin"
            )
            .as_slice(),
            false,
        ),
        (
            "seq22_auto_inventory_gui",
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter3_seq22_auto_inventory_gui_20260524_legacy.bin"
            )
            .as_slice(),
            include_bytes!(
                "../../../fixtures/live_object/local_xp2_chapter3_seq22_auto_inventory_gui_20260524_ee.bin"
            )
            .as_slice(),
            true,
        ),
    ] {
        let mut payload = legacy.to_vec();

        assert!(
            super::claim_payload_if_verified(&payload).is_none(),
            "{name} raw XP2 Chapter 3 stream should document the legacy Diamond shape"
        );

        let claim = rewrite_payload_to_exact_claim_for_test(&mut payload);
        assert_rewrite_matches_expected_or_current_player_semantics(
            name,
            legacy,
            payload.as_slice(),
            expected_ee,
            "{name} rewrite should match the harness-dumped EE bytes"
        );
        assert!(
            claim.records_examined >= 1,
            "{name} should retain at least one typed live-object record"
        );
        if expect_gui {
            assert!(
                claim.live_gui_item_create_records + claim.live_gui_read_buffer_records >= 1,
                "{name} should retain GUI live-object row ownership"
            );
        }
        assert_eq!(claim.declared, payload.len() - claim.fragment_bytes);
    }
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_xp2_chapter2_cutscene_invisibility_000f_rewrites_to_exact_shape() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_xp2_chapter2_cutscene_invisibility_u5_000f_20260522.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw XP2 cutscene-invisibility stream should document the unclaimed 0x000F Diamond shape"
    );

    assert!(
        crate::translate::m_frame::rewrite_live_object_payload_to_exact_ee_for_test(
            &mut payload,
            None
        ),
        "0x000F status/action stream should rewrite through the bounded creature adapter"
    );
    let claim = super::claim_payload_if_verified(&payload)
        .expect("rewritten 0x000F status/action stream should exact-claim");
    assert!(
        claim.creature_update_records >= 2,
        "fixture should retain the 0x000F update plus following visibility update"
    );
    assert_eq!(claim.inventory_records, 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_cepv22_missing_creature_appearance_is_lifecycle_safe() {
    // Local CEP v2.2 startup capture from 2026-05-20. The packet is already an
    // exact EE `P 05 01` read-window, but the `P/5` creature-appearance record
    // references Tavern Patron id `0x80000074` before this proxy has observed an
    // `A/5` for that id. EE `sub_14077FE10` logs a missing pCreature, then keeps
    // reading the appearance/name/equipment body with writes guarded by the
    // resolved pointer, so this exact record must remain cursor-safe instead of
    // being removed or quarantined as a generic missing-object `U`.
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_cepv22_seq13_creature_appearance_missing_add_20260520.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("CEP v2.2 creature appearance packet should be exact EE shape");
    let tavern_patron_appearance = claim
        .mentions
        .iter()
        .find(|mention| {
            mention.opcode == b'P'
                && mention.object_type == super::CREATURE_OBJECT_TYPE
                && mention.object_id == 0x8000_0074
        })
        .expect("fixture should contain the missing-add Tavern Patron appearance");
    assert!(!tavern_patron_appearance.requires_materialized_object);

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false)
        .expect("exact P/5 appearance records are cursor-safe without prior materialization");
    assert!(lifecycle_claim.creature_appearance_records >= 1);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_cepv22_seq11_zero_declared_stream_rewrites_and_claims_exactly() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_cepv22_seq11_liveobject_zero_declared_stream_20260520.bin"
    )
    .to_vec();

    crate::translate::live_object::normalize_prefixed_fragments_payload_if_needed(&mut payload)
        .expect("zero-declared CEP live-object stream should normalize");
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let exact_claim = super::claim_payload_if_verified(&payload)
        .expect("zero-declared CEP live-object stream should claim exactly");
    assert!(exact_claim.add_records >= 1);
    assert!(exact_claim.creature_appearance_records >= 1);
    assert!(exact_claim.creature_update_records >= 1);

    assert!(
        super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_none(),
        "the CEP stream contains adjacent exact but unmaterialized Diamond P/5 and U/5 no-op records"
    );
    let lifecycle_rewrite =
        super::remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, _| false)
            .expect("exact missing-object P/5 and U/5 should be removable after boundary proof");
    assert_eq!(lifecycle_rewrite.removed_update_records, 2);
    assert_eq!(lifecycle_rewrite.diamond_missing_object_update_records, 1);
    assert_eq!(
        lifecycle_rewrite.diamond_missing_object_appearance_records,
        1
    );

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false)
        .expect("zero-declared CEP stream should be lifecycle-safe after cleanup");
    assert!(lifecycle_claim.add_records >= 1);
    assert!(lifecycle_claim.creature_appearance_records >= 1);
    assert!(lifecycle_claim.creature_update_records >= 1);
    let accepted = include_bytes!(
        "../../../fixtures/live_object/local_cepv22_seq11_liveobject_claimed_20260520.bin"
    );
    let accepted_claim = super::claim_payload_if_verified_with_lifecycle(accepted, |_, _| false)
        .expect("accepted CEP stream fixture should remain lifecycle-safe");
    assert_eq!(accepted_claim.add_records, lifecycle_claim.add_records);
    assert_eq!(
        accepted_claim.creature_appearance_records,
        lifecycle_claim.creature_appearance_records
    );
    assert_eq!(
        accepted_claim.creature_update_records,
        lifecycle_claim.creature_update_records
    );
    assert_eq!(
        &payload[..lifecycle_claim.declared],
        &accepted[..accepted_claim.declared],
        "CEP seq11 cleanup should keep the same exact EE live-object read window emitted by the harness"
    );
}

#[test]
fn local_diamond_seq12_door_placeable_stream_claims_exact_shape_before_lifecycle_cleanup() {
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260517_rewritten.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("local Diamond seq12 door/placeable stream should claim after exact rewrite");

    assert_eq!(claim.add_records, 5);
    assert_eq!(
        claim.update_records, 6,
        "pre-cleanup compact stream still carries the orphan Diamond update"
    );
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::DOOR_OBJECT_TYPE
            && mention.object_id == 3
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_0006
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_000E
            && mention.requires_materialized_object
    }));
    assert!(
        super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false).is_none(),
        "compact seq12 fixture documents the orphan Diamond update before lifecycle cleanup"
    );
}

#[test]
fn local_diamond_seq12_compact_door_ids_canonicalize_to_ee_external_namespace() {
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260517_rewritten.bin"
    )
    .to_vec();

    let summary = super::canonicalize_compact_external_object_ids_payload_for_ee(&mut payload)
        .expect("seq12 compact Diamond door ids should canonicalize for EE");

    assert_eq!(summary.compact_add_ids_observed, 3);
    assert_eq!(summary.add_ids_rewritten, 3);
    assert_eq!(summary.reference_ids_rewritten, 3);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("canonicalized payload remains exact EE live-object shape");
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A' && mention.object_type == 0x0A && mention.object_id == 0x8000_0003
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == 0x0A
            && mention.object_id == 0x8000_0003
            && mention.requires_materialized_object
    }));
    assert!(
        super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_none(),
        "canonicalization proves the EE object namespace but does not remove the orphan Diamond update"
    );

    let cleanup =
        super::remove_unmaterialized_update_records_payload_if_possible(&mut payload, |_, _| false)
            .expect("canonicalized orphan Diamond update should be removable after exact proof");
    assert_eq!(cleanup.removed_update_records, 1);
    assert!(
        super::claim_payload_if_verified_with_lifecycle(&payload, |_, _| false).is_some(),
        "canonicalized payload must lifecycle-proof after the bounded missing-object cleanup"
    );
}

#[test]
fn local_diamond_seq12_rebuilt_high_bit_door_placeable_stream_claims_exactly() {
    // Local Diamond bridge capture from 2026-05-18, dumped after the
    // live-object high-level fragment stream was rebuilt and canonicalized for
    // EE.  It is deliberately separate from the compact-id 2026-05-17 fixture:
    // this pins the emitted EE-facing external-object namespace shape.
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_diamond_seq12_door_placeable_stream_20260518_claimed.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("rebuilt local Diamond seq12 stream should claim as exact EE live-object shape");
    assert_eq!(claim.add_records, 5);
    assert_eq!(claim.update_records, 5);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::DOOR_OBJECT_TYPE
            && mention.object_id == 0x8000_0003
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'A'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_0006
    }));
    assert!(claim.mentions.iter().any(|mention| {
        mention.opcode == b'U'
            && mention.object_type == super::PLACEABLE_OBJECT_TYPE
            && mention.object_id == 0x8000_000E
            && mention.requires_materialized_object
    }));

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false)
        .expect("add/update pairs in the same rebuilt stream should lifecycle-proof internally");
    assert_eq!(lifecycle_claim.add_records, 5);
    assert_eq!(lifecycle_claim.update_records, 5);
}

#[cfg(hgbridge_private_fixtures)]
#[test]
fn local_contest_character_sheet_gs_live_object_claims_exactly() {
    // Local Contest Of Champions strict capture from 2026-05-23. Opening the
    // character sheet emits a `G S` live GUI row with mask 0xF17F. EE's
    // `CNWSMessage::WriteGameObjUpdate_CharacterSheet` / client
    // `sub_1407B2740` own the nested combat-info and effect-icon fragment bits;
    // the strict validator must consume both the 57-byte read window and all 83
    // post-header fragment bits before this packet can leave quarantine.
    let payload = include_bytes!(
        "../../../fixtures/live_object/local_contest_character_sheet_gs_live_object_20260523.bin"
    );

    let claim = super::claim_payload_if_verified(payload)
        .expect("Contest character-sheet G/S live-object row should exact-claim");
    assert_eq!(claim.live_bytes_length, 57);
    assert_eq!(claim.fragment_bytes, 11);
    assert_eq!(claim.live_gui_read_buffer_records, 1);
    assert_eq!(claim.live_gui_item_create_records, 0);
    assert_eq!(claim.live_gui_fragment_bits, 83);
    assert_eq!(claim.records_examined, 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}

#[test]
fn hg_live_empty_live_object_shell_claims_as_exact_noop() {
    // Live Higher Ground driver-only capture from 2026-05-17.  This is a
    // GameObjUpdate_LiveObject high-level packet whose declared length ends at
    // the CNW header and whose fragment stream contains only the three
    // final-bit header bits (`0x60` => valid bit count 3).  The EE client
    // decompile path (`MessageMoreDataToRead` before submessage dispatch)
    // treats this as no live-object work to read, so the strict translator
    // claims it as a typed semantic no-op instead of quarantining it as an
    // unverified lifecycle packet.
    let payload =
        include_bytes!("../../../fixtures/live_object/hg_live_empty_live_object_noop_20260517.bin");

    let claim =
        super::claim_payload_if_verified(payload).expect("empty live-object shell should claim");
    assert_eq!(claim.declared, 7);
    assert_eq!(claim.live_bytes_length, 0);
    assert_eq!(claim.fragment_bytes, 1);
    assert_eq!(claim.records_examined, 0);
    assert!(claim.mentions.is_empty());

    let lifecycle_claim = super::claim_payload_if_verified_with_lifecycle(payload, |_, _| false)
        .expect("empty no-op shell should not require object lifecycle state");
    assert_eq!(lifecycle_claim.records_examined, 0);
    assert!(lifecycle_claim.mentions.is_empty());
}

#[test]
fn hg_starc5_seq34_current_player_inventory_gq_stream_claims_exactly() {
    // Live HG Starcore5 2026-05-18 seq34: current-player inventory owner
    // 0xFFFFFFFE with mask 0x2E00, followed by a GQ row stream and P/U creature
    // records. Diamond's inventory traversal uses this current-player sentinel;
    // the proxy must classify it through the exact inventory/GQ parser instead
    // of waiting on retries or treating the zlib window as raw traffic.
    let mut payload = include_bytes!(
        "../../../fixtures/live_object/hg_starc5_live_seq34_current_player_inventory_gq_20260518.bin"
    )
    .to_vec();

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "raw Diamond seq34 stream must not be claimed before typed rewrites"
    );

    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);
    let _ = super::rewrite_add_name_fragment_bits_payload_if_possible(&mut payload);
    let _ = crate::translate::live_object::rewrite_creature_add_visual_transform_maps_if_possible(
        &mut payload,
        None,
    );
    let _ = super::rewrite_update_records_payload_if_possible(&mut payload);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("HG seq34 current-player I/0x2E00 + GQ stream should rewrite to exact EE shape");

    assert_eq!(claim.inventory_records, 1);
    assert_eq!(claim.inventory_fragment_bits, 19);
    assert!(claim.live_gui_read_buffer_records >= 1);
    assert_eq!(claim.live_bytes_length + 7, claim.declared);
}
