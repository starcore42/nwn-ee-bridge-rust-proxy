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
