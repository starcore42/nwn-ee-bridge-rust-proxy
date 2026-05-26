//! Fixture-free live-object update regression anchors.

fn creature_status_effect_4008_payload(rows: &[(u16, Option<[u8; 3]>)]) -> Vec<u8> {
    let mut live = Vec::new();
    live.extend_from_slice(&[b'U', 0x05, 0x55, 0x00, 0x00, 0x80]);
    live.extend_from_slice(&0x0000_4008u32.to_le_bytes());
    live.extend_from_slice(&(rows.len() as u16).to_le_bytes());
    for (row, compact_target_payload) in rows {
        live.push(b'A');
        live.extend_from_slice(&row.to_le_bytes());
        if let Some(payload) = compact_target_payload {
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
fn creature_status_effect_single_compact_target_payload_is_exact_ee_shape() {
    let payload = creature_status_effect_4008_payload(&[(0x1234, Some([0x44, 0x33, 0x22]))]);

    let claim = super::claim_payload_if_verified(&payload)
        .expect("single compact-target status-effect row should be exact-owned");
    assert_eq!(claim.creature_update_records, 1);
}

#[test]
fn creature_status_effect_multi_compact_target_payload_stays_unclaimed_without_2da() {
    let payload = creature_status_effect_4008_payload(&[
        (0x1234, Some([0x44, 0x33, 0x22])),
        (0x1235, Some([0x55, 0x44, 0x33])),
    ]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da row-type proof, multi-row target payload boundaries are ambiguous"
    );
}

#[test]
fn creature_status_effect_mixed_target_payload_rows_stay_unclaimed_without_2da() {
    let payload =
        creature_status_effect_4008_payload(&[(0x00F3, None), (0x1234, Some([0x44, 0x33, 0x22]))]);

    assert!(
        super::claim_payload_if_verified(&payload).is_none(),
        "without visualeffects.2da row-type proof, mixed target/no-target rows cannot be exact-owned"
    );
}

#[test]
fn creature_interleaved_fragment_span_requires_exact_bit_cursor() {
    let mut live = vec![b'U', 0x05, 0x55, 0x00, 0x00, 0x80];
    live.extend_from_slice(&0x0000_C408u32.to_le_bytes());
    live.extend_from_slice(&0u16.to_le_bytes());
    live.extend_from_slice(&[0; 8]);
    let read_end = live.len();
    let span = super::bits::pack_msb_valid_bits(
        vec![false, false, false, true, false, true],
        super::CNW_FRAGMENT_HEADER_BITS,
    );
    live.extend_from_slice(&span);
    let old_record_end = live.len();
    let mut record_end = old_record_end;

    let mut fragment_bits = vec![false; super::CNW_FRAGMENT_HEADER_BITS + 10];
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
