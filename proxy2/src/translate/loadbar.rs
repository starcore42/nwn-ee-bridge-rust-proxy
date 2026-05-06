//! EE `LoadBar_*` payload construction.
//!
//! This module answers one narrow question:
//! "Given a verified semantic load-bar event, what exact EE-facing high-level
//! payload bytes should be emitted?"
//!
//! Decompile anchors used for these payloads:
//!
//! - EE `CNWSMessage::SendServerToPlayerLoadBar_StartStallEvent` creates a
//!   CNW write message, writes one 32-bit stall-event id, obtains the write
//!   buffer, then sends high-level family `0x2C` minor `0x01`.
//! - EE `CNWSMessage::SendServerToPlayerLoadBar_EndStallEvent` writes the same
//!   32-bit stall-event id followed by a 4-bit result code, then sends
//!   high-level family `0x2C` minor `0x03`.
//! - EE `CNWSMessage::SendServerToPlayerLoadBar_UpdateStallEvent` writes the
//!   stall-event id and a 32-bit progress value, then sends minor `0x02`.

const HIGH_LEVEL_ENVELOPE: u8 = b'P';
const LOADBAR_MAJOR: u8 = 0x2C;
const LOADBAR_START_MINOR: u8 = 0x01;
const LOADBAR_UPDATE_MINOR: u8 = 0x02;
const LOADBAR_END_MINOR: u8 = 0x03;

const LOADBAR_ONE_DWORD_DECLARED: u32 = 0x0B;
const LOADBAR_TWO_DWORD_DECLARED: u32 = 0x0F;

const START_FRAGMENT_BYTE: u8 = 0x60;
const END_SUCCESS_FRAGMENT_BYTE: u8 = 0xE0;
const UPDATE_FRAGMENT_BYTE: u8 = 0xE0;

pub fn start_payload(stall_event_id: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    payload.push(HIGH_LEVEL_ENVELOPE);
    payload.push(LOADBAR_MAJOR);
    payload.push(LOADBAR_START_MINOR);
    payload.extend_from_slice(&LOADBAR_ONE_DWORD_DECLARED.to_le_bytes());
    payload.extend_from_slice(&stall_event_id.to_le_bytes());
    payload.push(START_FRAGMENT_BYTE);
    payload
}

pub fn end_success_payload(stall_event_id: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(12);
    payload.push(HIGH_LEVEL_ENVELOPE);
    payload.push(LOADBAR_MAJOR);
    payload.push(LOADBAR_END_MINOR);
    payload.extend_from_slice(&LOADBAR_ONE_DWORD_DECLARED.to_le_bytes());
    payload.extend_from_slice(&stall_event_id.to_le_bytes());
    payload.push(END_SUCCESS_FRAGMENT_BYTE);
    payload
}

#[allow(dead_code)]
pub fn update_payload(stall_event_id: u32, progress: u32) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16);
    payload.push(HIGH_LEVEL_ENVELOPE);
    payload.push(LOADBAR_MAJOR);
    payload.push(LOADBAR_UPDATE_MINOR);
    payload.extend_from_slice(&LOADBAR_TWO_DWORD_DECLARED.to_le_bytes());
    payload.extend_from_slice(&stall_event_id.to_le_bytes());
    payload.extend_from_slice(&progress.to_le_bytes());
    payload.push(UPDATE_FRAGMENT_BYTE);
    payload
}
