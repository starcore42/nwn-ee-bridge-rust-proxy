//! Packet-level live-object update regression anchors.

#[test]
fn legacy_facing_zero_encodes_to_zero_ee_scalar() {
    assert_eq!(super::writer::encode_ee_scalar_orientation_from_legacy_facing(0), 0);
}

#[test]
fn legacy_facing_wraps_inside_ee_scalar_range() {
    assert!(super::writer::encode_ee_scalar_orientation_from_legacy_facing(u16::MAX) <= 0x0FFF);
}
