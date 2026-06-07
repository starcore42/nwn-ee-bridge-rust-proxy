//! CNW fragment-bit helpers for live-object update rewrites.
//!
//! These helpers know only the NWN MSB-first fragment bit packing. Door,
//! placeable, and trigger semantics stay in their own modules.

pub(super) fn decode_msb_valid_bits(fragment: &[u8], min_valid_bits: usize) -> Option<Vec<bool>> {
    let first = *fragment.first()?;
    let final_fragment_bits = ((first & 0xE0) >> 5) as usize;
    let valid_bits = if final_fragment_bits == 0 {
        fragment.len().checked_mul(8)?
    } else {
        fragment
            .len()
            .checked_sub(1)?
            .checked_mul(8)?
            .checked_add(final_fragment_bits)?
    };
    if valid_bits < min_valid_bits {
        return None;
    }

    let mut bits = Vec::with_capacity(valid_bits);
    for bit_index in 0..valid_bits {
        let byte = *fragment.get(bit_index / 8)?;
        bits.push((byte & (0x80 >> (bit_index % 8))) != 0);
    }
    Some(bits)
}

pub(super) fn pack_msb_valid_bits(mut bits: Vec<bool>, header_bits: usize) -> Vec<u8> {
    if bits.len() < header_bits {
        return Vec::new();
    }
    let final_fragment_bits = bits.len() % 8;
    bits[0] = (final_fragment_bits & 0x04) != 0;
    bits[1] = (final_fragment_bits & 0x02) != 0;
    bits[2] = (final_fragment_bits & 0x01) != 0;

    let mut packed = vec![0u8; bits.len().div_ceil(8)];
    for (bit_index, bit) in bits.into_iter().enumerate() {
        if bit {
            packed[bit_index / 8] |= 0x80 >> (bit_index % 8);
        }
    }
    packed
}

pub(super) fn insert_msb_bit(bits: &mut Vec<bool>, bit_index: usize, value: bool) -> Option<()> {
    if bit_index > bits.len() {
        return None;
    }
    bits.insert(bit_index, value);
    Some(())
}

pub(super) fn insert_msb_bits(
    bits: &mut Vec<bool>,
    bit_index: usize,
    values: &[bool],
) -> Option<()> {
    if bit_index > bits.len() {
        return None;
    }
    for (index, value) in values.iter().copied().enumerate() {
        bits.insert(bit_index + index, value);
    }
    Some(())
}

pub(super) fn erase_msb_bits(bits: &mut Vec<bool>, bit_index: usize, count: usize) -> Option<()> {
    if bit_index > bits.len() || bits.len().saturating_sub(bit_index) < count {
        return None;
    }
    bits.drain(bit_index..bit_index + count);
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_header_uses_three_msb_valid_count_bits_before_payload() {
        // Diamond `CreateWriteMessage` (`nwserver` 0x507E30) and EE
        // `CreateWriteMessage` (`nwn` 0x1402D54A0) reserve the first three MSB
        // bits before live-object payload bits. `GetWriteMessage` later stores
        // the final-byte valid-bit count there; semantic record bits start at
        // cursor 3.
        let payload_bits = [
            true, false, false, true, true, false, false, true, false, true, false,
        ];
        let mut bits = vec![false; 3];
        bits.extend_from_slice(&payload_bits);

        let packed = pack_msb_valid_bits(bits, 3);
        assert_eq!(packed, [0xD3, 0x28]);

        let decoded =
            decode_msb_valid_bits(&packed, 3).expect("packed fragment should decode exactly");
        assert_eq!(&decoded[..3], &[true, true, false]);
        assert_eq!(&decoded[3..], payload_bits);
    }

    #[test]
    fn fragment_header_repack_does_not_create_semantic_prefix_bits() {
        // Diamond `WriteBOOL` (`nwserver` 0x507FC0 -> 0x507340) and EE
        // `WriteBOOL` (`nwn` 0x1402DA920 -> 0x1402DB990) advance the same
        // MSB-first cursor. The header rewrite may change bits 0..3, but it
        // cannot donate one or two extra source bits before the next live
        // object record. This pins the CEP-style U/6 handoff risk as a real
        // semantic cursor question, not a CNW final-count artifact.
        let next_record_bits = [
            false, true, true, true, false, true, false, true, true, false, false, false, false,
            false,
        ];
        let mut bits = vec![true, true, true];
        bits.extend_from_slice(&next_record_bits);

        let packed = pack_msb_valid_bits(bits, 3);
        let decoded =
            decode_msb_valid_bits(&packed, 3).expect("repacked fragment should decode exactly");

        assert_eq!(
            (packed[0] & 0xE0) >> 5,
            ((3 + next_record_bits.len()) % 8) as u8,
            "only the reserved header bits carry the final-byte valid-bit count"
        );
        assert_eq!(
            &decoded[3..],
            next_record_bits,
            "semantic bits must remain at cursor 3 after final-count repack"
        );
    }
}
