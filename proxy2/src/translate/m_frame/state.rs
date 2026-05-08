//! Typed M-frame session substates.
//!
//! Keep the root M-frame dispatcher from becoming a god-state container. Each
//! substate below owns one transport concern, so future packet-family work has
//! an obvious home instead of casually adding fields to `SessionState`.

use flate2::Decompress;

use crate::translate::{ContinuationOwner, area, module_resources};

use super::{
    live_stream, quickbar_stream,
    reassembly::{CompletedDeflatedStreamWindow, ServerDeflatedReassembly},
    sequence::SequenceShift,
    synthetic_area,
};

#[derive(Debug, Default)]
pub(super) struct DeflateState {
    pub(super) server_reassembly: Option<ServerDeflatedReassembly>,
    pub(super) server_zlib_inflater: Option<Decompress>,
    pub(super) completed_server_stream_windows: Vec<CompletedDeflatedStreamWindow>,
    pub(super) server_zlib_stream_proxy_owned: bool,
    pub(super) server_zlib_stream_owner: Option<ContinuationOwner>,
}

#[derive(Debug, Default)]
pub(super) struct QuickbarStreamState {
    pub(super) pending_stream: Option<quickbar_stream::PendingQuickbarStream>,
}

#[derive(Debug, Default)]
pub(super) struct LiveObjectStreamState {
    pub(super) pending_stream: Option<live_stream::PendingLiveObjectStream>,
}

#[derive(Debug, Default)]
pub(super) struct SequenceState {
    pub(super) latest_client_sequence_from_client: Option<u16>,
    pub(super) latest_client_ack_from_client: Option<u16>,
    pub(super) client_sequence_shifts: Vec<SequenceShift>,
    pub(super) server_sequence_shifts: Vec<SequenceShift>,
}

#[derive(Debug, Default)]
pub(super) struct SyntheticAreaState {
    pub(super) pending_server_to_client_packets: Vec<synthetic_area::PendingServerPacket>,
    pub(super) pending_area_loaded: Option<synthetic_area::PendingAreaLoaded>,
}

#[derive(Debug, Default)]
pub(super) struct AreaContextState {
    pub(super) latest_area_placeables: area::AreaPlaceableContext,
}

#[derive(Debug, Default)]
pub struct SessionState {
    pub(super) deflate: DeflateState,
    pub(super) quickbar: QuickbarStreamState,
    pub(super) live_object: LiveObjectStreamState,
    pub(super) sequence: SequenceState,
    pub(super) synthetic_area: SyntheticAreaState,
    pub(super) area_context: AreaContextState,
    pub(super) module_resources: module_resources::ModuleResourceRuntime,
}

impl SessionState {
    pub fn new(module_resources: module_resources::ModuleResourceRuntime) -> Self {
        Self {
            module_resources,
            ..Self::default()
        }
    }
}
