//! Module time update semantic claims.
//!
//! EE `CNWSMessage::SendServerToPlayerModuleUpdate_Time` writes family
//! `0x03`, minor `0x03` with a CNW read window that starts with a BYTE update
//! mask. The decompile then conditionally writes BYTE/DWORD fields according
//! to bits `0x01`, `0x02`, `0x04`, `0x08`, and `0x10`. The 1.69/HG captures
//! use the same mask-driven shape, so this module validates the cursor walk and
//! claims the packet as an intentional no-op translation. The reader consumes
//! no CNW BOOLs, so any trailing fragment storage is owned only as the optional
//! empty final cursor byte written by `GetWriteMessage`.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const MODULE_MAJOR: u8 = 0x03;
const MODULE_TIME_MINOR: u8 = 0x03;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const MAX_FRAGMENT_BYTES: usize = 1;
const KNOWN_TIME_MASK_BITS: u8 = 0x1F;
const CNW_FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const EMPTY_CNW_FRAGMENT_CURSOR: u8 = 0x60;

#[derive(Debug, Clone, Copy)]
pub struct ModuleTimeClaimSummary {
    pub mask: u8,
    pub declared: usize,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ModuleTimeClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != MODULE_MAJOR
        || high.minor != MODULE_TIME_MINOR
        || payload.len() < READ_START + 1
    {
        return None;
    }

    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START + 1
        || declared > payload.len()
        || payload.len().saturating_sub(declared) > MAX_FRAGMENT_BYTES
    {
        return None;
    }

    let mut cursor = READ_START;
    let mask = *payload.get(cursor)?;
    cursor += 1;
    if (mask & !KNOWN_TIME_MASK_BITS) != 0 {
        return None;
    }
    if (mask & 0x01) != 0 {
        let subkind = *payload.get(cursor)?;
        cursor += 1;
        if matches!(subkind, 3 | 4) {
            cursor = cursor.checked_add(4)?;
        }
    }
    if (mask & 0x02) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x04) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x08) != 0 {
        cursor = cursor.checked_add(1)?;
    }
    if (mask & 0x10) != 0 {
        cursor = cursor.checked_add(4)?;
    }
    if cursor != declared || !empty_fragment_tail_valid(&payload[declared..]) {
        return None;
    }

    Some(ModuleTimeClaimSummary {
        mask,
        declared,
        fragment_bytes: payload.len() - declared,
    })
}

fn empty_fragment_tail_valid(fragment_tail: &[u8]) -> bool {
    match fragment_tail {
        [] => true,
        [byte] => (byte & CNW_FRAGMENT_CURSOR_MASK) == EMPTY_CNW_FRAGMENT_CURSOR,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_module_time(mask: u8, body: &[u8], tail: &[u8]) -> Vec<u8> {
        let declared = (READ_START + 1 + body.len()) as u32;
        let mut payload = vec![b'P', MODULE_MAJOR, MODULE_TIME_MINOR];
        payload.extend_from_slice(&declared.to_le_bytes());
        payload.push(mask);
        payload.extend_from_slice(body);
        payload.extend_from_slice(tail);
        payload
    }

    #[test]
    fn claims_zero_mask_module_time_with_empty_fragment_cursor() {
        let payload = build_module_time(0, &[], &[EMPTY_CNW_FRAGMENT_CURSOR]);

        let claim = claim_payload_if_verified(&payload)
            .expect("zero-mask Module_Time still owns the exact cursor");
        assert_eq!(claim.mask, 0);
        assert_eq!(claim.fragment_bytes, 1);
    }

    #[test]
    fn rejects_module_time_fragment_bits_and_tail_slack() {
        let with_data_bits = build_module_time(0x02, &[0x12], &[0x80]);
        assert!(
            claim_payload_if_verified(&with_data_bits).is_none(),
            "Module_Time reads no BOOLs, so data fragment bits are unowned"
        );

        let with_slack = build_module_time(0x02, &[0x12], &[EMPTY_CNW_FRAGMENT_CURSOR, 0]);
        assert!(
            claim_payload_if_verified(&with_slack).is_none(),
            "only one optional empty final cursor byte is proven"
        );
    }
}
