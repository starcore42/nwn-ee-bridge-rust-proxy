// CNW fragment bit reader for quickbar item objects. CNW BOOLs are packed
// least-significant-bit first; the first three bits encode the final fragment
// bit count used by the original reader as an end-of-stream guard.

#[derive(Debug, Clone)]
struct QuickbarPacketReader<'a> {
    read_buffer: &'a [u8],
    fragments: &'a [u8],
    cursor: usize,
    fragment_cursor: usize,
    fragment_bit: u8,
    final_fragment_bits: u8,
}

impl<'a> QuickbarPacketReader<'a> {
    fn read_bit(&mut self) -> Option<bool> {
        let byte = *self.fragments.get(self.fragment_cursor)?;
        let bit = ((byte >> self.fragment_bit) & 1) != 0;
        self.fragment_bit = self.fragment_bit.saturating_add(1);
        if self.fragment_bit == 8 {
            self.fragment_bit = 0;
            self.fragment_cursor = self.fragment_cursor.checked_add(1)?;
        }
        Some(bit)
    }

    fn read_bits(&mut self, count: u8) -> Option<u32> {
        let mut value = 0u32;
        for shift in 0..count {
            if self.read_bit()? {
                value |= 1u32 << shift;
            }
        }
        Some(value)
    }

    fn read_byte(&mut self) -> Option<u8> {
        let byte = *self.read_buffer.get(self.cursor)?;
        self.cursor = self.cursor.checked_add(1)?;
        Some(byte)
    }

    fn read_word(&mut self) -> Option<u16> {
        let value = read_u16_le(self.read_buffer, self.cursor)?;
        self.cursor = self.cursor.checked_add(2)?;
        Some(value)
    }

    fn read_dword(&mut self) -> Option<u32> {
        let value = read_u32_le(self.read_buffer, self.cursor)?;
        self.cursor = self.cursor.checked_add(CNW_LENGTH_BYTES)?;
        Some(value)
    }

    fn read_i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.read_dword()?.to_le_bytes()))
    }

    fn read_string(&mut self) -> Option<Vec<u8>> {
        let len = usize::try_from(self.read_dword()?).ok()?;
        if len > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
            return None;
        }
        let end = self.cursor.checked_add(len)?;
        let text = self.read_buffer.get(self.cursor..end)?.to_vec();
        self.cursor = end;
        Some(text)
    }

    fn read_loc_string(&mut self) -> Option<QuickbarLocStringField> {
        let custom_tlk = self.read_bit()?;
        if custom_tlk {
            let language_selector = self.read_bit()?;
            let string_ref = self.read_dword()?;
            Some(QuickbarLocStringField {
                custom_tlk,
                language_selector,
                string_ref,
                text: Vec::new(),
            })
        } else {
            let text = self.read_string()?;
            Some(QuickbarLocStringField {
                custom_tlk,
                language_selector: false,
                string_ref: 0,
                text,
            })
        }
    }

    fn skip_bytes(&mut self, count: usize) -> Option<()> {
        self.cursor = self.cursor.checked_add(count)?;
        if self.cursor > self.read_buffer.len() {
            return None;
        }
        Some(())
    }

    fn skip_string(&mut self) -> Option<()> {
        let len = usize::try_from(self.read_dword()?).ok()?;
        if len > MAX_REASONABLE_QUICKBAR_STRING_BYTES {
            return None;
        }
        self.skip_bytes(len)
    }

    fn skip_loc_string(&mut self) -> Option<()> {
        if self.read_bit()? {
            let _language_selector = self.read_bit()?;
            let _string_ref = self.read_dword()?;
            Some(())
        } else {
            self.skip_string()
        }
    }
}
