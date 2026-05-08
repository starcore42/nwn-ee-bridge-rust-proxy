// Byte-order helpers local to quickbar parsing/writing.

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

fn read_u16_le(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(slice))
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) -> Option<()> {
    let target = bytes.get_mut(offset..offset.checked_add(4)?)?;
    target.copy_from_slice(&value.to_le_bytes());
    Some(())
}
