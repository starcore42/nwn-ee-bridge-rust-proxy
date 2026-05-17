//! Decompile-backed visual-transform wire helpers.
//!
//! EE has two related but distinct transform encodings in live-object traffic:
//!
//! * `ObjectVisualTransformData::Write` writes the object-level scoped transform
//!   map. For EE players satisfying build `2001/0x23`, the identity value is an
//!   empty map and therefore serializes as two 32-bit zero counts. The matching
//!   client reader is the routine currently identified as `sub_140973160`.
//! * `CAurObjectVisualTransformData` is the legacy per-scope transform payload.
//!   Its old scalar identity representation is ten 32-bit floats, but that is
//!   not the object-level map shape expected by the EE client on modern builds.
//!
//! Keeping these bytes named here avoids the old trap where "identity visual
//! transform" could silently mean two different packet shapes.

pub(crate) const EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN: usize = 8;
pub(crate) const EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8;
    EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] = [0; EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN];

pub(crate) const LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN: usize = 40;
pub(crate) const LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES: [u8;
    LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] = [
    0x00, 0x00, 0x80, 0x3F, // scale x
    0x00, 0x00, 0x80, 0x3F, // scale y
    0x00, 0x00, 0x80, 0x3F, // scale z
    0x00, 0x00, 0x00, 0x00, // translation x
    0x00, 0x00, 0x00, 0x00, // translation y
    0x00, 0x00, 0x00, 0x00, // translation z
    0x00, 0x00, 0x00, 0x00, // rotation x
    0x00, 0x00, 0x00, 0x00, // rotation y
    0x00, 0x00, 0x00, 0x00, // rotation z
    0x00, 0x00, 0x80, 0x3F, // alpha
];

pub(crate) fn has_ee_object_visual_transform_identity_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let Some(end) = offset.checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN) else {
        return false;
    };
    end <= record_end && bytes.get(offset..end) == Some(&EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES)
}

pub(crate) fn has_legacy_scalar_visual_transform_identity_at(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
) -> bool {
    let Some(end) = offset.checked_add(LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN) else {
        return false;
    };
    end <= record_end
        && bytes.get(offset..end) == Some(&LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES)
}

pub(crate) fn replace_legacy_scalar_identity_with_ee_object_identity(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: usize,
) -> Option<usize> {
    if !has_legacy_scalar_visual_transform_identity_at(bytes, offset, record_end) {
        return None;
    }

    let end = offset.checked_add(LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    bytes.splice(offset..end, EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    Some(LEGACY_SCALAR_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)
}

pub(crate) fn insert_ee_object_visual_transform_identity(
    bytes: &mut Vec<u8>,
    offset: usize,
    record_end: &mut usize,
) -> Option<usize> {
    if offset != *record_end {
        return None;
    }

    bytes.splice(offset..offset, EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
    *record_end = (*record_end).checked_add(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)?;
    Some(EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN)
}
