pub fn legacy_m_crc(bytes: &[u8]) -> Option<u16> {
    // Decompile-backed rule:
    // `CNetLayerInternal::CRCEncodeFrame` / `CRCVerifyFrame` require an `M`
    // frame of at least 0x0C bytes, seed the CRC accumulator with zero, run
    // the reflected 0xEDB88320 polynomial over bytes 3..end, and store the
    // low 16 bits big-endian in bytes 1..2. The first Rust bridge draft used
    // a placeholder additive checksum here, which correctly caused strict mode
    // to quarantine every gameplay frame after BN auth. Keep this deliberately
    // explicit so future packet rewrites repair the same value EE verifies.
    if bytes.len() < 12 || bytes.first().copied()? != b'M' {
        return None;
    }

    let mut table = [0u32; 256];
    for (index, value) in table.iter_mut().enumerate() {
        let mut crc = index as u32;
        for _ in 0..8 {
            crc = if (crc & 1) != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
        *value = crc;
    }

    let mut crc = 0u32;
    for byte in &bytes[3..] {
        crc = (crc >> 8) ^ table[((crc ^ u32::from(*byte)) & 0xff) as usize];
    }
    Some((crc & 0xffff) as u16)
}

pub fn read_be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice = bytes.get(offset..offset + 2)?;
    Some(u16::from_be_bytes([slice[0], slice[1]]))
}

pub fn write_be_u16(bytes: &mut [u8], offset: usize, value: u16) -> bool {
    let Some(slice) = bytes.get_mut(offset..offset + 2) else {
        return false;
    };
    let [high, low] = value.to_be_bytes();
    slice[0] = high;
    slice[1] = low;
    true
}

pub fn encode_legacy_m_crc(bytes: &mut [u8]) -> bool {
    let Some(crc) = legacy_m_crc(bytes) else {
        return false;
    };
    write_be_u16(bytes, 1, crc)
}

pub fn read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}
