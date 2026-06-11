//! EE live-object update writers.

use super::{
    EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES, EE_UPDATE_SCALE_STATE_READ_BYTES,
    LEGACY_UPDATE_ORIENTATION_MASK, LEGACY_UPDATE_SCALE_STATE_MASK, reader::LegacyNamedUpdateTail,
};

pub(super) fn build_ee_door_placeable_generic_update_bytes(
    legacy_tail: LegacyNamedUpdateTail,
    translated_mask: u32,
) -> Vec<u8> {
    let mut rewritten = Vec::with_capacity(
        EE_UPDATE_ORIENTATION_SCALAR_READ_BYTES + EE_UPDATE_SCALE_STATE_READ_BYTES,
    );
    if (translated_mask & LEGACY_UPDATE_ORIENTATION_MASK) != 0 {
        let scalar12 = encode_ee_scalar_orientation_from_legacy_facing(legacy_tail.facing);
        rewritten.push(((scalar12 >> 4) & 0xFF) as u8);
    }
    if (translated_mask & LEGACY_UPDATE_SCALE_STATE_MASK) != 0 {
        rewritten.extend_from_slice(&legacy_tail.scale_raw.to_le_bytes());
        rewritten.extend_from_slice(&legacy_tail.generic_state_word.to_le_bytes());
    }
    rewritten
}

pub(super) fn encode_ee_scalar_orientation_from_legacy_facing(facing: u16) -> u16 {
    const LEGACY_FULL_TURN_UNITS: f64 = 65536.0;
    const FULL_TURN_DEGREES: f64 = 360.0;
    const EE_TENTHS_PER_DEGREE: f64 = 10.0;

    let legacy_degrees = f64::from(facing) * FULL_TURN_DEGREES / LEGACY_FULL_TURN_UNITS;
    // Decompile-backed orientation rule:
    // - Diamond `sub_467AE0` and EE `sub_14079C050` both read the generic
    //   object orientation mask as BOOL vector_mode followed by the compact
    //   scalar branch when that BOOL is false.
    // - Both readers decode the 12-bit tenths-degree scalar, then apply the
    //   same model-basis conversion by adding +90 degrees (`+pi/2`) before
    //   storing the orientation vector.
    //
    // The old injected hook wrote runtime vectors directly after bypassing the
    // packet reader, so it used cos/sin(facing).  The proxy feeds EE's packet
    // reader, and Diamond/EE agree on the scalar packet basis.  Pre-subtracting
    // 90 degrees double-corrects the packet value and leaves doors, signs, and
    // placeables one quadrant away from their frames.
    let raw = (legacy_degrees * EE_TENTHS_PER_DEGREE + 0.000001).floor() as u32;
    raw.min(0x0FFF) as u16
}

pub(super) fn encode_ee_scalar_orientation_from_bearing_radians(bearing: f32) -> Option<u16> {
    if !bearing.is_finite() {
        return None;
    }
    const FULL_TURN_TENTHS: i64 = 3600;
    let normalized = f64::from(bearing).rem_euclid(std::f64::consts::TAU);
    let raw = (normalized * FULL_TURN_TENTHS as f64 / std::f64::consts::TAU).round() as i64;
    Some(raw.rem_euclid(FULL_TURN_TENTHS).min(0x0FFF) as u16)
}

#[cfg(test)]
mod tests {
    use super::{
        encode_ee_scalar_orientation_from_bearing_radians,
        encode_ee_scalar_orientation_from_legacy_facing,
    };

    #[test]
    fn ee_scalar_orientation_preserves_shared_diamond_ee_packet_basis_for_cardinals() {
        assert_eq!(encode_ee_scalar_orientation_from_legacy_facing(0x0000), 0);
        assert_eq!(encode_ee_scalar_orientation_from_legacy_facing(0x4000), 900);
        assert_eq!(
            encode_ee_scalar_orientation_from_legacy_facing(0x8000),
            1800
        );
        assert_eq!(
            encode_ee_scalar_orientation_from_legacy_facing(0xC000),
            2700
        );
    }

    #[test]
    fn ee_scalar_orientation_preserves_static_area_bearing_basis() {
        assert_eq!(
            encode_ee_scalar_orientation_from_bearing_radians(0.0),
            Some(0)
        );
        assert_eq!(
            encode_ee_scalar_orientation_from_bearing_radians(std::f32::consts::FRAC_PI_2),
            Some(900)
        );
        assert_eq!(
            encode_ee_scalar_orientation_from_bearing_radians(std::f32::consts::PI),
            Some(1800)
        );
        assert_eq!(
            encode_ee_scalar_orientation_from_bearing_radians(-std::f32::consts::FRAC_PI_2),
            Some(2700)
        );
    }
}
