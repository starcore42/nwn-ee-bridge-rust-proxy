//! Client-originated EE device-property semantic ownership.
//!
//! EE advertises local renderer/UI properties through `Device_AdvertiseProperty`
//! (`0x36/0x01`) before and during the pregame flow. Diamond/1.69 has no
//! matching handler, so the bridge consumes these reliable frames as proxy-owned
//! empty progress carriers. The packet still needs exact ownership so mixed
//! client streams can remove only the EE-only unit.
//!
//! Decompile/C++ parity:
//! - High-level envelope `70 36 01`.
//! - CNW declared read-buffer length at payload offset 3.
//! - Read buffer then contains `CExoString property_name`, `DWORD flag`, and
//!   `DWORD value` only when `flag == 1`.
//! - The reader consumes no fragment BOOLs; at most one optional
//!   `GetWriteMessage` empty final cursor byte may follow the declared window.

use crate::{crc::read_le_u32, packet::m::HighLevel};

const DEVICE_MAJOR: u8 = 0x36;
const ADVERTISE_PROPERTY_MINOR: u8 = 0x01;
const HIGH_LEVEL_HEADER_BYTES: usize = 3;
const CNW_LENGTH_BYTES: usize = 4;
const READ_START: usize = HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES;
const CEXO_STRING_LENGTH_BYTES: usize = 4;
const DWORD_BYTES: usize = 4;
const MAX_DEVICE_PROPERTY_NAME_BYTES: usize = 256;
const CNW_FRAGMENT_CURSOR_MASK: u8 = 0xE0;
const EMPTY_CNW_FRAGMENT_CURSOR: u8 = 0x60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientDeviceClaimSummary {
    pub packet_name: &'static str,
    pub declared: usize,
    pub property_name_len: usize,
    pub flag: u32,
    pub has_value: bool,
    pub fragment_bytes: usize,
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientDeviceClaimSummary> {
    let high = HighLevel::parse(payload)?;
    if high.major != DEVICE_MAJOR || high.minor != ADVERTISE_PROPERTY_MINOR {
        return None;
    }

    let shape = device_advertise_property_shape(payload)?;
    Some(ClientDeviceClaimSummary {
        packet_name: "Device_AdvertiseProperty",
        declared: shape.declared,
        property_name_len: shape.property_name_len,
        flag: shape.flag,
        has_value: shape.has_value,
        fragment_bytes: payload.len().checked_sub(shape.declared)?,
    })
}

pub fn unit_end_if_verified(bytes: &[u8]) -> Option<usize> {
    let high = HighLevel::parse(bytes)?;
    if high.major != DEVICE_MAJOR || high.minor != ADVERTISE_PROPERTY_MINOR {
        return None;
    }

    let read_end = device_advertise_property_read_end(bytes)?;
    if read_end == bytes.len() || boundary_has_high_level(bytes, read_end) {
        return Some(read_end);
    }

    let fragment_end = read_end.checked_add(1)?;
    if fragment_end <= bytes.len()
        && empty_fragment_tail_valid(&bytes[read_end..fragment_end])
        && (fragment_end == bytes.len() || boundary_has_high_level(bytes, fragment_end))
    {
        return Some(fragment_end);
    }

    None
}

#[derive(Debug, Clone, Copy)]
struct DeviceAdvertisePropertyShape {
    declared: usize,
    property_name_len: usize,
    flag: u32,
    has_value: bool,
}

fn device_advertise_property_shape(payload: &[u8]) -> Option<DeviceAdvertisePropertyShape> {
    let declared = device_advertise_property_read_end(payload)?;
    if !empty_fragment_tail_valid(payload.get(declared..)?) {
        return None;
    }
    parse_device_advertise_property_read_window(payload, declared)
}

fn device_advertise_property_read_end(payload: &[u8]) -> Option<usize> {
    let declared = usize::try_from(read_le_u32(payload, HIGH_LEVEL_HEADER_BYTES)?).ok()?;
    if declared < READ_START + CEXO_STRING_LENGTH_BYTES + DWORD_BYTES || declared > payload.len() {
        return None;
    }
    parse_device_advertise_property_read_window(payload, declared).map(|_| declared)
}

fn parse_device_advertise_property_read_window(
    payload: &[u8],
    declared: usize,
) -> Option<DeviceAdvertisePropertyShape> {
    let mut cursor = READ_START;
    let property_name_len = usize::try_from(read_le_u32(payload, cursor)?).ok()?;
    cursor = cursor.checked_add(CEXO_STRING_LENGTH_BYTES)?;
    if property_name_len > MAX_DEVICE_PROPERTY_NAME_BYTES {
        return None;
    }
    cursor = cursor.checked_add(property_name_len)?;
    if cursor.checked_add(DWORD_BYTES)? > declared {
        return None;
    }
    let flag = read_le_u32(payload, cursor)?;
    cursor = cursor.checked_add(DWORD_BYTES)?;
    let has_value = flag == 1;
    if has_value {
        cursor = cursor.checked_add(DWORD_BYTES)?;
    }
    if cursor != declared {
        return None;
    }

    Some(DeviceAdvertisePropertyShape {
        declared,
        property_name_len,
        flag,
        has_value,
    })
}

fn empty_fragment_tail_valid(fragment_tail: &[u8]) -> bool {
    match fragment_tail {
        [] => true,
        [byte] => (byte & CNW_FRAGMENT_CURSOR_MASK) == EMPTY_CNW_FRAGMENT_CURSOR,
        _ => false,
    }
}

fn boundary_has_high_level(bytes: &[u8], offset: usize) -> bool {
    offset < bytes.len() && HighLevel::parse(&bytes[offset..]).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device_payload(name: &str, flag: u32, value: Option<u32>, tail: &[u8]) -> Vec<u8> {
        let mut payload = vec![0x70, DEVICE_MAJOR, ADVERTISE_PROPERTY_MINOR];
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&(name.len() as u32).to_le_bytes());
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&flag.to_le_bytes());
        if let Some(value) = value {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        let declared = payload.len() as u32;
        payload[HIGH_LEVEL_HEADER_BYTES..HIGH_LEVEL_HEADER_BYTES + CNW_LENGTH_BYTES]
            .copy_from_slice(&declared.to_le_bytes());
        payload.extend_from_slice(tail);
        payload
    }

    #[test]
    fn claims_declared_device_advertise_property_with_value() {
        let payload = device_payload(
            "graphics.windowed",
            1,
            Some(0),
            &[EMPTY_CNW_FRAGMENT_CURSOR],
        );

        let claim = claim_payload_if_verified(&payload)
            .expect("declared Device_AdvertiseProperty should be claimed");

        assert_eq!(claim.packet_name, "Device_AdvertiseProperty");
        assert_eq!(claim.property_name_len, "graphics.windowed".len());
        assert_eq!(claim.flag, 1);
        assert!(claim.has_value);
        assert_eq!(claim.fragment_bytes, 1);
        assert_eq!(unit_end_if_verified(&payload), Some(payload.len()));
    }

    #[test]
    fn claims_declared_device_advertise_property_without_value() {
        let payload = device_payload("ui.scale", 0, None, &[]);

        let claim = claim_payload_if_verified(&payload)
            .expect("flag-zero Device_AdvertiseProperty should own no value DWORD");

        assert_eq!(claim.property_name_len, "ui.scale".len());
        assert_eq!(claim.flag, 0);
        assert!(!claim.has_value);
        assert_eq!(claim.fragment_bytes, 0);
    }

    #[test]
    fn rejects_old_undeclared_device_shape() {
        let mut payload = vec![0x70, DEVICE_MAJOR, ADVERTISE_PROPERTY_MINOR];
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.push(b'x');
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&0u32.to_le_bytes());

        assert!(claim_payload_if_verified(&payload).is_none());
    }

    #[test]
    fn unit_end_stops_before_following_high_level() {
        let mut stream = device_payload("gamma", 1, Some(1), &[EMPTY_CNW_FRAGMENT_CURSOR]);
        let device_end = stream.len();
        stream.extend_from_slice(&[0x70, 0x11, 0x01]);

        assert_eq!(unit_end_if_verified(&stream), Some(device_end));
    }

    #[test]
    fn rejects_fragment_tail_with_data_bits() {
        let payload = device_payload("gamma", 1, Some(1), &[0x80]);

        assert!(claim_payload_if_verified(&payload).is_none());
        assert!(unit_end_if_verified(&payload).is_none());
    }
}
