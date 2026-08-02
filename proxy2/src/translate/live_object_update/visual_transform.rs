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
    EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN] =
    [0; EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN];

const EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS: usize = 10;
const MAX_EE_OBJECT_VISUAL_TRANSFORM_MAP_ENTRIES: usize = 4096;

/// Current-EE (`2001/0x23`) `LerpFloat` wire value.
///
/// EE client `sub_140973450` reads one raw FLOAT, then two signed INT32 values.
/// A zero second INT ends the value. A nonzero second INT guards three more raw
/// FLOATs followed by the two build-`0x23` INT32 fields. All of those values
/// live in the byte stream; the helper consumes no CNW fragment BOOLs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EeLerpFloatWireValue {
    pub current_float_bits: u32,
    pub first_timeline_value: i32,
    pub second_timeline_value: i32,
    pub active: Option<EeActiveLerpFloatWireValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EeActiveLerpFloatWireValue {
    pub first_float_bits: u32,
    pub second_float_bits: u32,
    pub third_float_bits: u32,
    pub first_build_23_value: i32,
    pub second_build_23_value: i32,
}

/// One value-bearing entry from the second keyed list read by
/// `ObjectVisualTransformData::Read` (`sub_140973160`).
///
/// `sub_140972C70` first consumes one MSB-first CNW BOOL. True selects the
/// identity value and owns no bytes. False reads exactly ten `LerpFloat`
/// values in scale/vector/alpha storage order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EeAurObjectVisualTransformWireValue {
    Identity,
    LerpValues(Box<[EeLerpFloatWireValue; EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS]>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EeObjectVisualTransformValueEntry {
    pub scope: i32,
    pub value: EeAurObjectVisualTransformWireValue,
}

/// Current-EE object-level visual-transform map in the exact server-writer
/// shape.
///
/// EE client `sub_140973160` reads a signed INT32 count and that many signed
/// INT32 keys, then a second signed INT32 count and repeated
/// `key + CAurObjectVisualTransformData` entries. The authoritative
/// `ObjectVisualTransformData::Write` at `0x14059683C..0x140596900` writes the
/// same `std::map<int, ...>` size and signed-ascending keys in both passes.
/// Exact proxy validation therefore requires equal counts, identical keys,
/// strict signed ordering, and uniqueness instead of accepting every looser
/// shape the client reader happens to tolerate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct EeObjectVisualTransformMap {
    pub entries: Vec<EeObjectVisualTransformValueEntry>,
}

impl EeObjectVisualTransformMap {
    pub(crate) fn is_canonical_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParsedEeObjectVisualTransformMap {
    pub map: EeObjectVisualTransformMap,
    pub end: usize,
    pub fragment_bits_consumed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EncodedEeObjectVisualTransformMap {
    pub bytes: Vec<u8>,
    /// BOOLs in the exact logical order consumed by `CNWMessage::ReadBOOL`.
    /// The enclosing live-object writer owns their MSB-first byte packing.
    pub fragment_bits: Vec<bool>,
}

/// Parse the current-EE map at one decompile-owned byte/bit cursor.
///
/// The entry cap bounds adversarial allocation independently of the enclosing
/// packet limit. A value-bearing entry cannot be parsed without exact fragment
/// bits because its first BOOL chooses between the zero-byte identity branch
/// and the ten-value byte branch.
pub(crate) fn parse_ee_object_visual_transform_map(
    bytes: &[u8],
    offset: usize,
    record_end: usize,
    fragment_bits: Option<&[bool]>,
    fragment_bit_cursor: usize,
) -> Option<ParsedEeObjectVisualTransformMap> {
    if offset > record_end || record_end > bytes.len() {
        return None;
    }

    let mut cursor = offset;
    let key_count = read_nonnegative_bounded_count(bytes, &mut cursor, record_end)?;
    let mut keys = Vec::with_capacity(key_count);
    for _ in 0..key_count {
        let key = read_i32_le_at_cursor(bytes, &mut cursor, record_end)?;
        if keys.last().is_some_and(|previous| *previous >= key) {
            return None;
        }
        keys.push(key);
    }

    let value_count = read_nonnegative_bounded_count(bytes, &mut cursor, record_end)?;
    if value_count != key_count {
        return None;
    }
    let mut value_entries = Vec::with_capacity(value_count);
    let mut fragment_bits_consumed = 0usize;
    for expected_scope in keys {
        let scope = read_i32_le_at_cursor(bytes, &mut cursor, record_end)?;
        if scope != expected_scope {
            return None;
        }
        let identity =
            *fragment_bits?.get(fragment_bit_cursor.checked_add(fragment_bits_consumed)?)?;
        fragment_bits_consumed = fragment_bits_consumed.checked_add(1)?;
        let value = if identity {
            EeAurObjectVisualTransformWireValue::Identity
        } else {
            let mut values = Vec::with_capacity(EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS);
            for _ in 0..EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS {
                values.push(parse_ee_lerp_float(bytes, &mut cursor, record_end)?);
            }
            EeAurObjectVisualTransformWireValue::LerpValues(Box::new(values.try_into().ok()?))
        };
        value_entries.push(EeObjectVisualTransformValueEntry { scope, value });
    }

    Some(ParsedEeObjectVisualTransformMap {
        map: EeObjectVisualTransformMap {
            entries: value_entries,
        },
        end: cursor,
        fragment_bits_consumed,
    })
}

/// Encode the exact inverse of the current-EE reader above.
///
/// Byte fields remain little-endian raw values and fragment BOOLs remain
/// unpacked so the enclosing CNW writer can insert them at its already-proven
/// shared bit cursor without resetting or byte-aligning it.
pub(crate) fn encode_ee_object_visual_transform_map(
    map: &EeObjectVisualTransformMap,
) -> Option<EncodedEeObjectVisualTransformMap> {
    if map.entries.len() > MAX_EE_OBJECT_VISUAL_TRANSFORM_MAP_ENTRIES {
        return None;
    }
    if map
        .entries
        .windows(2)
        .any(|pair| pair[0].scope >= pair[1].scope)
    {
        return None;
    }

    let mut bytes = Vec::new();
    let mut fragment_bits = Vec::with_capacity(map.entries.len());
    write_count(&mut bytes, map.entries.len())?;
    for entry in &map.entries {
        bytes.extend_from_slice(&entry.scope.to_le_bytes());
    }
    write_count(&mut bytes, map.entries.len())?;
    for entry in &map.entries {
        bytes.extend_from_slice(&entry.scope.to_le_bytes());
        match &entry.value {
            EeAurObjectVisualTransformWireValue::Identity => fragment_bits.push(true),
            EeAurObjectVisualTransformWireValue::LerpValues(values) => {
                fragment_bits.push(false);
                for value in values.iter() {
                    write_ee_lerp_float(&mut bytes, value)?;
                }
            }
        }
    }

    Some(EncodedEeObjectVisualTransformMap {
        bytes,
        fragment_bits,
    })
}

fn read_nonnegative_bounded_count(
    bytes: &[u8],
    cursor: &mut usize,
    record_end: usize,
) -> Option<usize> {
    let count = usize::try_from(read_i32_le_at_cursor(bytes, cursor, record_end)?).ok()?;
    (count <= MAX_EE_OBJECT_VISUAL_TRANSFORM_MAP_ENTRIES).then_some(count)
}

fn read_i32_le_at_cursor(bytes: &[u8], cursor: &mut usize, record_end: usize) -> Option<i32> {
    let end = cursor.checked_add(4)?;
    let raw = bytes.get(*cursor..end)?;
    if end > record_end {
        return None;
    }
    *cursor = end;
    Some(i32::from_le_bytes(raw.try_into().ok()?))
}

fn read_u32_le_at_cursor(bytes: &[u8], cursor: &mut usize, record_end: usize) -> Option<u32> {
    let end = cursor.checked_add(4)?;
    let raw = bytes.get(*cursor..end)?;
    if end > record_end {
        return None;
    }
    *cursor = end;
    Some(u32::from_le_bytes(raw.try_into().ok()?))
}

fn parse_ee_lerp_float(
    bytes: &[u8],
    cursor: &mut usize,
    record_end: usize,
) -> Option<EeLerpFloatWireValue> {
    let current_float_bits = read_u32_le_at_cursor(bytes, cursor, record_end)?;
    let first_timeline_value = read_i32_le_at_cursor(bytes, cursor, record_end)?;
    let second_timeline_value = read_i32_le_at_cursor(bytes, cursor, record_end)?;
    let active = if second_timeline_value == 0 {
        None
    } else {
        Some(EeActiveLerpFloatWireValue {
            first_float_bits: read_u32_le_at_cursor(bytes, cursor, record_end)?,
            second_float_bits: read_u32_le_at_cursor(bytes, cursor, record_end)?,
            third_float_bits: read_u32_le_at_cursor(bytes, cursor, record_end)?,
            first_build_23_value: read_i32_le_at_cursor(bytes, cursor, record_end)?,
            second_build_23_value: read_i32_le_at_cursor(bytes, cursor, record_end)?,
        })
    };
    Some(EeLerpFloatWireValue {
        current_float_bits,
        first_timeline_value,
        second_timeline_value,
        active,
    })
}

fn write_count(bytes: &mut Vec<u8>, count: usize) -> Option<()> {
    let count = i32::try_from(count).ok()?;
    bytes.extend_from_slice(&count.to_le_bytes());
    Some(())
}

fn write_ee_lerp_float(bytes: &mut Vec<u8>, value: &EeLerpFloatWireValue) -> Option<()> {
    if value.active.is_some() != (value.second_timeline_value != 0) {
        return None;
    }
    bytes.extend_from_slice(&value.current_float_bits.to_le_bytes());
    bytes.extend_from_slice(&value.first_timeline_value.to_le_bytes());
    bytes.extend_from_slice(&value.second_timeline_value.to_le_bytes());
    if let Some(active) = value.active {
        bytes.extend_from_slice(&active.first_float_bits.to_le_bytes());
        bytes.extend_from_slice(&active.second_float_bits.to_le_bytes());
        bytes.extend_from_slice(&active.third_float_bits.to_le_bytes());
        bytes.extend_from_slice(&active.first_build_23_value.to_le_bytes());
        bytes.extend_from_slice(&active.second_build_23_value.to_le_bytes());
    }
    Some(())
}

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
    parse_ee_object_visual_transform_map(bytes, offset, record_end, None, 0).is_some_and(|parsed| {
        parsed.map.is_canonical_empty()
            && parsed.fragment_bits_consumed == 0
            && parsed.end == offset + EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES_LEN
    })
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
    let encoded = encode_ee_object_visual_transform_map(&EeObjectVisualTransformMap::default())?;
    if !encoded.fragment_bits.is_empty() {
        return None;
    }
    bytes.splice(offset..end, encoded.bytes);
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

    let encoded = encode_ee_object_visual_transform_map(&EeObjectVisualTransformMap::default())?;
    if !encoded.fragment_bits.is_empty()
        || encoded.bytes.as_slice() != EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES
    {
        return None;
    }
    let inserted = encoded.bytes.len();
    bytes.splice(offset..offset, encoded.bytes);
    *record_end = (*record_end).checked_add(inserted)?;
    Some(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inactive_lerp(current_float_bits: u32, first_timeline_value: i32) -> EeLerpFloatWireValue {
        EeLerpFloatWireValue {
            current_float_bits,
            first_timeline_value,
            second_timeline_value: 0,
            active: None,
        }
    }

    #[test]
    fn canonical_empty_object_map_is_exact_two_count_writer_contract() {
        let map = EeObjectVisualTransformMap::default();
        let encoded = encode_ee_object_visual_transform_map(&map).expect("empty map encodes");
        assert_eq!(encoded.bytes, EE_OBJECT_VISUAL_TRANSFORM_IDENTITY_BYTES);
        assert!(encoded.fragment_bits.is_empty());

        let parsed =
            parse_ee_object_visual_transform_map(&encoded.bytes, 0, encoded.bytes.len(), None, 0)
                .expect("empty map parses without a fragment proof");
        assert_eq!(parsed.map, map);
        assert_eq!(parsed.end, encoded.bytes.len());
        assert_eq!(parsed.fragment_bits_consumed, 0);
    }

    #[test]
    fn keyed_identity_and_value_entries_round_trip_without_reordering() {
        let mut lerps = [inactive_lerp(0, 0); EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS];
        lerps[0] = inactive_lerp(1.0f32.to_bits(), -7);
        lerps[9] = EeLerpFloatWireValue {
            current_float_bits: 0.5f32.to_bits(),
            first_timeline_value: 11,
            second_timeline_value: 12,
            active: Some(EeActiveLerpFloatWireValue {
                first_float_bits: 1.25f32.to_bits(),
                second_float_bits: (-2.5f32).to_bits(),
                third_float_bits: 3.75f32.to_bits(),
                first_build_23_value: -13,
                second_build_23_value: 14,
            }),
        };
        let map = EeObjectVisualTransformMap {
            entries: vec![
                EeObjectVisualTransformValueEntry {
                    scope: -9,
                    value: EeAurObjectVisualTransformWireValue::Identity,
                },
                EeObjectVisualTransformValueEntry {
                    scope: 42,
                    value: EeAurObjectVisualTransformWireValue::LerpValues(Box::new(lerps)),
                },
            ],
        };

        let encoded = encode_ee_object_visual_transform_map(&map).expect("general map encodes");
        assert_eq!(encoded.fragment_bits, [true, false]);
        let mut surrounding_bits = vec![false, true, false];
        surrounding_bits.extend_from_slice(&encoded.fragment_bits);
        surrounding_bits.push(true);
        let parsed = parse_ee_object_visual_transform_map(
            &encoded.bytes,
            0,
            encoded.bytes.len(),
            Some(&surrounding_bits),
            3,
        )
        .expect("general map parses from a nonzero shared bit cursor");
        assert_eq!(parsed.map, map);
        assert_eq!(parsed.end, encoded.bytes.len());
        assert_eq!(parsed.fragment_bits_consumed, 2);
        assert_eq!(
            encode_ee_object_visual_transform_map(&parsed.map),
            Some(encoded),
            "the typed reader/writer must preserve key order and raw scalar bits"
        );
    }

    #[test]
    fn value_entries_require_their_guard_bool_and_complete_branch_bytes() {
        let map = EeObjectVisualTransformMap {
            entries: vec![EeObjectVisualTransformValueEntry {
                scope: 5,
                value: EeAurObjectVisualTransformWireValue::LerpValues(Box::new(
                    [inactive_lerp(0, 0); EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS],
                )),
            }],
        };
        let encoded = encode_ee_object_visual_transform_map(&map).expect("value map encodes");
        assert!(
            parse_ee_object_visual_transform_map(&encoded.bytes, 0, encoded.bytes.len(), None, 0,)
                .is_none(),
            "the byte shape alone cannot choose identity versus ten-value BOOL branches"
        );
        let mut truncated = encoded.bytes.clone();
        truncated.pop();
        assert!(
            parse_ee_object_visual_transform_map(
                &truncated,
                0,
                truncated.len(),
                Some(&encoded.fragment_bits),
                0,
            )
            .is_none(),
            "a false guard owns all ten complete LerpFloat values"
        );
    }

    #[test]
    fn exact_writer_contract_rejects_mismatched_unsorted_and_duplicate_key_lists() {
        let map = EeObjectVisualTransformMap {
            entries: vec![
                EeObjectVisualTransformValueEntry {
                    scope: 2,
                    value: EeAurObjectVisualTransformWireValue::Identity,
                },
                EeObjectVisualTransformValueEntry {
                    scope: 1,
                    value: EeAurObjectVisualTransformWireValue::Identity,
                },
            ],
        };
        assert!(encode_ee_object_visual_transform_map(&map).is_none());

        let bits = [true, true];
        let mut mismatched = Vec::new();
        mismatched.extend_from_slice(&2i32.to_le_bytes());
        mismatched.extend_from_slice(&1i32.to_le_bytes());
        mismatched.extend_from_slice(&2i32.to_le_bytes());
        mismatched.extend_from_slice(&2i32.to_le_bytes());
        mismatched.extend_from_slice(&1i32.to_le_bytes());
        mismatched.extend_from_slice(&3i32.to_le_bytes());
        assert!(
            parse_ee_object_visual_transform_map(&mismatched, 0, mismatched.len(), Some(&bits), 0,)
                .is_none(),
            "the client reader is permissive, but exact server-writer output repeats identical ordered keys"
        );

        let mut duplicate = Vec::new();
        duplicate.extend_from_slice(&2i32.to_le_bytes());
        duplicate.extend_from_slice(&1i32.to_le_bytes());
        duplicate.extend_from_slice(&1i32.to_le_bytes());
        duplicate.extend_from_slice(&2i32.to_le_bytes());
        duplicate.extend_from_slice(&1i32.to_le_bytes());
        duplicate.extend_from_slice(&1i32.to_le_bytes());
        assert!(
            parse_ee_object_visual_transform_map(&duplicate, 0, duplicate.len(), Some(&bits), 0,)
                .is_none(),
            "std::map output has strictly increasing unique signed keys"
        );
    }

    #[test]
    fn writer_rejects_lerp_payloads_that_disagree_with_the_guard_field() {
        let mut values = [inactive_lerp(0, 0); EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS];
        values[0].second_timeline_value = 1;
        let map = EeObjectVisualTransformMap {
            entries: vec![EeObjectVisualTransformValueEntry {
                scope: 1,
                value: EeAurObjectVisualTransformWireValue::LerpValues(Box::new(values)),
            }],
        };
        assert!(encode_ee_object_visual_transform_map(&map).is_none());

        let mut values = [inactive_lerp(0, 0); EE_OBJECT_VISUAL_TRANSFORM_LERP_FLOATS];
        values[0].active = Some(EeActiveLerpFloatWireValue {
            first_float_bits: 0,
            second_float_bits: 0,
            third_float_bits: 0,
            first_build_23_value: 0,
            second_build_23_value: 0,
        });
        let map = EeObjectVisualTransformMap {
            entries: vec![EeObjectVisualTransformValueEntry {
                scope: 1,
                value: EeAurObjectVisualTransformWireValue::LerpValues(Box::new(values)),
            }],
        };
        assert!(encode_ee_object_visual_transform_map(&map).is_none());
    }
}
