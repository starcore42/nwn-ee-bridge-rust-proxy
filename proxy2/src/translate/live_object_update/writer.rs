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
    const EE_READER_BASIS_DEGREES: f64 = 90.0;
    const EE_TENTHS_PER_DEGREE: f64 = 10.0;

    let legacy_degrees = f64::from(facing) * FULL_TURN_DEGREES / LEGACY_FULL_TURN_UNITS;
    // Decompile-backed orientation rule:
    // - EE server `WriteGameObjUpdate_UpdateObject` computes `Yaw(Vector)` in
    //   degrees, writes a scalar-orientation branch BOOL `false`, then emits
    //   `CNWMessage::WriteFLOAT(yaw_degrees, 10.0f, 12)`.
    // - `CNWMessage::WriteFLOAT(value, scale, bits)` writes
    //   `floor(value * scale)`, and `ReadFLOAT(scale, bits)` returns
    //   `raw / scale`.
    // - The EE/Diamond generic door/placeable update reader then applies its
    //   model-basis conversion by adding +90 degrees (`+pi/2`) before storing
    //   the orientation vector.
    //
    // HG's legacy anchored tail stores the pre-basis legacy facing turn. To
    // make EE end at that same facing after its reader adds the basis, the
    // packet scalar must be pre-compensated by -90 degrees here.
    let ee_packet_degrees =
        (legacy_degrees - EE_READER_BASIS_DEGREES).rem_euclid(FULL_TURN_DEGREES);
    let raw = (ee_packet_degrees * EE_TENTHS_PER_DEGREE + 0.000001).floor() as u32;
    raw.min(0x0FFF) as u16
}

#[cfg(test)]
mod tests {
    use super::encode_ee_scalar_orientation_from_legacy_facing;

    #[test]
    fn ee_scalar_orientation_precompensates_reader_basis_for_legacy_cardinals() {
        assert_eq!(encode_ee_scalar_orientation_from_legacy_facing(0x0000), 2700);
        assert_eq!(encode_ee_scalar_orientation_from_legacy_facing(0x4000), 0);
        assert_eq!(encode_ee_scalar_orientation_from_legacy_facing(0x8000), 900);
        assert_eq!(encode_ee_scalar_orientation_from_legacy_facing(0xC000), 1800);
    }
}
