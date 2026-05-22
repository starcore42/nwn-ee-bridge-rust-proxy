use crate::crc::{legacy_m_crc, read_be_u16, read_le_u32};

pub const LEGACY_GAMEPLAY_PAYLOAD_OFFSET: usize = 12;
pub const MAX_REASONABLE_GAMEPLAY_PAYLOAD: usize = 1 << 20;

#[derive(Debug, Clone)]
pub struct MFrame<'a> {
    pub bytes: &'a [u8],
    pub parsed: Option<MFrameView>,
}

impl<'a> MFrame<'a> {
    pub fn parse(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            parsed: MFrameView::parse(bytes),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MFrameView {
    pub crc: u16,
    pub computed_crc: u16,
    pub crc_valid: bool,
    pub sequence: u16,
    pub ack_sequence: u16,
    pub flags: u8,
    pub frame_type: u8,
    pub packetized_sequence: u16,
    pub declared_payload_length: usize,
    pub payload_length: usize,
    pub available_payload_length: usize,
    pub trailing_payload_length: usize,
    pub deflated_or_extended: bool,
    pub uses_extended_packet_length: bool,
    pub high: Option<HighLevel>,
    pub deflated: Option<DeflatedEnvelope>,
}

impl MFrameView {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < LEGACY_GAMEPLAY_PAYLOAD_OFFSET || bytes.first().copied()? != b'M' {
            return None;
        }

        let crc = read_be_u16(bytes, 1)?;
        let computed_crc = legacy_m_crc(bytes)?;
        let sequence = read_be_u16(bytes, 3)?;
        let ack_sequence = read_be_u16(bytes, 5)?;
        let flags = *bytes.get(7)?;
        let packetized_sequence = read_be_u16(bytes, 8)?;
        let frame_type = (flags >> 4) & 0x03;
        let uses_extended_packet_length = (flags & 0x80) != 0;
        let deflated_or_extended = (flags & 0x04) != 0;
        // The legacy window has a fixed 12-byte header. Decompile/C++ parity:
        // normal frames store the packetized payload length at bytes 10..11;
        // extended frames splice the high word into bytes 12..13 while the
        // payload window still begins at offset 12. We parse that odd shape
        // exactly, but strict validation will still reject declarations that
        // run beyond the datagram.
        let declared_payload_length = if uses_extended_packet_length {
            if bytes.len() < LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 2 {
                return None;
            }
            ((usize::from(bytes[12])) << 24)
                | ((usize::from(bytes[13])) << 16)
                | ((usize::from(bytes[10])) << 8)
                | usize::from(bytes[11])
        } else {
            read_be_u16(bytes, 10)? as usize
        };
        let payload_offset = LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        if payload_offset > bytes.len() {
            return None;
        }
        let available_payload_length = bytes.len().saturating_sub(payload_offset);
        // `0` is a sentinel in observed/decompiled reliable-window traffic:
        // the mature C++ proxy initializes payload length to the whole
        // remaining datagram and only narrows it when the packetized length is
        // nonzero and in bounds. Without this rule, valid startup/compressed
        // server frames look like zero-payload records with hundreds of
        // "trailing" bytes.
        let payload_length = if declared_payload_length != 0
            && declared_payload_length <= available_payload_length
        {
            declared_payload_length
        } else {
            available_payload_length
        };
        let payload = bytes.get(payload_offset..payload_offset + payload_length)?;
        let trailing_payload_length = available_payload_length.saturating_sub(payload_length);

        let high = HighLevel::parse(payload);
        let deflated = if deflated_or_extended && payload.len() >= 4 {
            let inflated_length = read_le_u32(payload, 0)? as usize;
            Some(DeflatedEnvelope {
                inflated_length,
                compressed_length: payload.len() - 4,
                plausible: inflated_length != 0
                    && inflated_length <= MAX_REASONABLE_GAMEPLAY_PAYLOAD
                    && payload.len() - 4 <= MAX_REASONABLE_GAMEPLAY_PAYLOAD,
            })
        } else {
            None
        };

        Some(Self {
            crc,
            computed_crc,
            crc_valid: crc == computed_crc,
            sequence,
            ack_sequence,
            flags,
            frame_type,
            packetized_sequence,
            declared_payload_length,
            payload_length,
            available_payload_length,
            trailing_payload_length,
            deflated_or_extended,
            uses_extended_packet_length,
            high,
            deflated,
        })
    }
}

/// A queued CNetLayerWindow record appended after the primary M payload.
///
/// EE's `CNetLayerWindow::FrameReceive` stores and drains individual window
/// frames through `CExoNetExtendableBuffer` / `LoadWindowWithFrames`, so a
/// single datagram can carry a primary high-level message followed by one or
/// more fixed-header packetized records. This parser is intentionally pure: it
/// only describes the record shape and leaves all allow/quarantine policy to
/// `strict`.
#[derive(Debug, Clone)]
pub struct PacketizedSpanView {
    pub offset: usize,
    pub flags: u8,
    pub packetized_sequence: u16,
    pub declared_payload_length: usize,
    pub payload_length: usize,
    pub record_length: usize,
    pub uses_extended_packet_length: bool,
    pub high: Option<HighLevel>,
    pub deflated: Option<DeflatedEnvelope>,
}

impl PacketizedSpanView {
    pub fn parse_at(bytes: &[u8], offset: usize) -> Option<Self> {
        let remaining = bytes.len().checked_sub(offset)?;
        if remaining < LEGACY_GAMEPLAY_PAYLOAD_OFFSET {
            return None;
        }

        let flags = *bytes.get(offset + 7)?;
        let packetized_sequence = read_be_u16(bytes, offset + 8)?;
        let uses_extended_packet_length = (flags & 0x80) != 0;
        let declared_payload_length = if uses_extended_packet_length {
            if remaining < LEGACY_GAMEPLAY_PAYLOAD_OFFSET + 2 {
                return None;
            }
            ((usize::from(bytes[offset + 12])) << 24)
                | ((usize::from(bytes[offset + 13])) << 16)
                | ((usize::from(bytes[offset + 10])) << 8)
                | usize::from(bytes[offset + 11])
        } else {
            read_be_u16(bytes, offset + 10)? as usize
        };

        if declared_payload_length > remaining - LEGACY_GAMEPLAY_PAYLOAD_OFFSET {
            return None;
        }

        let payload_offset = offset + LEGACY_GAMEPLAY_PAYLOAD_OFFSET;
        let payload_length = declared_payload_length;
        let payload = bytes.get(payload_offset..payload_offset + payload_length)?;
        let record_length = LEGACY_GAMEPLAY_PAYLOAD_OFFSET + payload_length;
        let high = HighLevel::parse(payload);
        let deflated = if (flags & 0x04) != 0 && payload.len() >= 4 {
            let inflated_length = read_le_u32(payload, 0)? as usize;
            Some(DeflatedEnvelope {
                inflated_length,
                compressed_length: payload.len() - 4,
                plausible: inflated_length != 0
                    && inflated_length <= MAX_REASONABLE_GAMEPLAY_PAYLOAD
                    && payload.len() - 4 <= MAX_REASONABLE_GAMEPLAY_PAYLOAD,
            })
        } else {
            None
        };

        Some(Self {
            offset,
            flags,
            packetized_sequence,
            declared_payload_length,
            payload_length,
            record_length,
            uses_extended_packet_length,
            high,
            deflated,
        })
    }
}

pub fn parse_packetized_spans(bytes: &[u8], mut offset: usize) -> Option<Vec<PacketizedSpanView>> {
    if offset > bytes.len() {
        return None;
    }

    let mut spans = Vec::new();
    while offset < bytes.len() {
        let span = PacketizedSpanView::parse_at(bytes, offset)?;
        if span.record_length == 0 {
            return None;
        }
        offset += span.record_length;
        spans.push(span);
    }
    Some(spans)
}

#[derive(Debug, Clone)]
pub struct DeflatedEnvelope {
    pub inflated_length: usize,
    pub compressed_length: usize,
    pub plausible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighLevel {
    pub envelope: u8,
    pub major: u8,
    pub minor: u8,
}

impl HighLevel {
    pub fn parse(payload: &[u8]) -> Option<Self> {
        let envelope = *payload.first()?;
        if envelope != b'P' && envelope != 0x70 {
            return None;
        }
        Some(Self {
            envelope,
            major: *payload.get(1)?,
            minor: *payload.get(2)?,
        })
    }

    pub fn name(self) -> &'static str {
        match (self.major, self.minor) {
            (0x01, 0x00) => "ServerStatus_0",
            (0x01, 0x01) => "ServerStatus_Status",
            (0x01, 0x03) => "ServerStatus_ModuleRunning",
            (0x02, 0x05) => "Login_Confirm",
            (0x02, 0x0A) => "Login_CharacterQuery",
            (0x02, 0x0C) => "Login_GetWaypoint",
            (0x02, 0x0D) => "Login_WaypointResponse",
            (0x02, 0x10) => "Login_NeedCharacter",
            (0x02, 0x11) => "Login_ServerSubDirectoryCharacter",
            (0x02, 0x12) => "Login_Fail",
            (0x03, 0x01) => "Module_Info",
            (0x03, 0x02) => "Module_Loaded",
            (0x03, 0x03) => "Module_Time",
            (0x03, 0x0E) => "Module_EndGame",
            (0x04, 0x01) => "Area_ClientArea",
            (0x04, 0x03) => "Area_AreaLoaded",
            (0x05, 0x01) => "GameObjUpdate_LiveObject",
            // EE packet-name table maps 0x0502 to GameObjUpdate_ObjControl.
            // `CNWSMessage::SendServerToPlayerGameObjUpdate_ObjControl`
            // decompiles to an 8-byte CNW write message: DWORD player id
            // plus WriteOBJECTIDServer(controlled object), sent as family 5,
            // minor 2. Strict validation still checks that exact shape before
            // allowing this packet through.
            (0x05, 0x02) => "GameObjUpdate_ObjControl",
            // EE packet-name table maps 0x0503 to GameObjUpdate_VisEffect.
            // `CNWSMessage::SendServerToPlayerGameObjUpdateVisEffect`
            // writes object id, visual-effect WORD, Vector floats, and
            // optional source/transform fields. The semantic translator owns
            // exact shape validation for each claimed branch.
            (0x05, 0x03) => "GameObjUpdate_VisEffect",
            (0x06, 0x01) => "Input_WalkToWaypoint",
            (0x06, 0x02) => "Input_Attack",
            (0x06, 0x03) => "Input_ChangeDoorState",
            (0x06, 0x04) => "Input_PlayAnimation",
            (0x06, 0x05) => "Input_Examine",
            (0x06, 0x06) => "Input_UseFeat",
            (0x06, 0x07) => "Input_UseSkill",
            (0x06, 0x08) => "Input_Dialog",
            (0x06, 0x09) => "Input_UseItem",
            (0x06, 0x0A) => "Input_ToggleMode",
            (0x06, 0x0B) => "Input_UseObject",
            (0x06, 0x0C) => "Input_UnlockObject",
            (0x06, 0x0D) => "Input_Rest",
            (0x06, 0x0E) => "Input_LockObject",
            (0x06, 0x0F) => "Input_StopDragMode",
            (0x06, 0x10) => "Input_MemorizeSpell",
            (0x06, 0x11) => "Input_UnMemorizeSpell",
            (0x06, 0x12) => "Input_CastSpell",
            // Chat family confirmed from EE's packet-name table:
            // 0x0904 = Chat_Tell, 0x0905 = Chat_ServerTell, 0x0907 =
            // Chat_AIAction_PlaySound, 0x0908..0x090A are Chat_StrRef
            // variants, and 0x090B/0x090C = Chat_TokenTalk variants. These are normal
            // CNWMessage-backed server-to-player payloads in the decompiled
            // chat senders, so strict mode may classify them as known only
            // when the focused chat translator proves the exact byte shape.
            (0x09, 0x04) => "Chat_Tell",
            (0x09, 0x05) => "Chat_ServerTell",
            (0x09, 0x07) => "Chat_AIAction_PlaySound",
            (0x09, 0x08) => "Chat_TalkRef",
            (0x09, 0x09) => "Chat_ShoutRef",
            (0x09, 0x0A) => "Chat_WhisperRef",
            (0x09, 0x0B) => "Chat_TokenTalk",
            (0x09, 0x0C) => "Chat_TokenTalkNoBubble",
            (0x0A, 0x01) => "PlayerList_All",
            (0x0A, 0x02) => "PlayerList_Add",
            // Inventory family confirmed from EE's packet-name table and
            // `CNWSMessage::SendServerToPlayerInventory_Equip`.
            (0x0C, 0x01) => "Inventory_Equip",
            // GUI inventory family confirmed from EE's packet-name table and
            // `CNWSMessage::SendPlayerToServerGuiInventory_Status` /
            // `HandlePlayerToServerGuiInventoryMessage`. Strict/client
            // translation owns the EE self-object-id compatibility rewrite.
            (0x0D, 0x01) => "GuiInventory_Status",
            (0x0D, 0x02) => "GuiInventory_SelectPanel",
            // Party family confirmed from EE's packet-name table:
            // 0x0E01..0x0E0E map to the party list/request/invite/control
            // messages, and the exported CNWSMessage senders include
            // SendServerToPlayerParty_List and
            // SendServerToPlayerParty_TransferObjectControl. Strict mode
            // still validates the CNW wrapper shape before allowing these.
            (0x0E, 0x01) => "Party_List",
            (0x0E, 0x02) => "Party_GetList",
            (0x0E, 0x03) => "Party_ListAdd",
            (0x0E, 0x04) => "Party_ListRemove",
            (0x0E, 0x05) => "Party_Join",
            (0x0E, 0x06) => "Party_Leave",
            (0x0E, 0x07) => "Party_Kick",
            (0x0E, 0x08) => "Party_TransferLeadership",
            (0x0E, 0x09) => "Party_Invite",
            (0x0E, 0x0A) => "Party_IgnoreInvitation",
            (0x0E, 0x0B) => "Party_AcceptInvitation",
            (0x0E, 0x0C) => "Party_RejectInvitation",
            (0x0E, 0x0D) => "Party_KickHenchman",
            (0x0E, 0x0E) => "Party_TransferObjectControl",
            // Camera family confirmed from EE's packet-name table and the
            // exported `CNWSMessage::SendServerToPlayerCamera_*` senders. The
            // semantic camera translator owns exact CNW cursor validation for
            // each claimed minor.
            (0x10, 0x01) => "Camera_ChangeLocation",
            (0x10, 0x02) => "Camera_SetMode",
            (0x10, 0x03) => "Camera_Store",
            (0x10, 0x04) => "Camera_Restore",
            (0x10, 0x05) => "Camera_SetHeight",
            (0x10, 0x06) => "Camera_LockPitch",
            (0x10, 0x07) => "Camera_LockDist",
            (0x10, 0x08) => "Camera_LockYaw",
            (0x10, 0x09) => "Camera_SetLimits",
            (0x10, 0x0A) => "Camera_Attach",
            (0x10, 0x0B) => "Camera_AttachRevert",
            (0x10, 0x0C) => "Camera_SetFlags",
            (0x11, 0x01) => "CharList_Request",
            (0x11, 0x02) => "CharList_ListResponse",
            (0x11, 0x03) => "CharList_RequestUpdateChar",
            (0x11, 0x04) => "CharList_UpdateCharResponse",
            (0x12, 0x0B) => "ClientSideMessage_Feedback",
            // Dialog family is byte-identical for the currently-owned local
            // Diamond shapes only after `translate::dialog` proves the exact
            // decompiled cursor model.  Major 0x14 is routed to Diamond's
            // dialog handler; `translate::dialog` owns each minor's exact CNW
            // cursor shape before strict mode treats the name as safe.
            (0x14, 0x01) => "Dialog_Entry",
            (0x14, 0x02) => "Dialog_Replies",
            (0x14, 0x03) => "Dialog_Reply",
            (0x14, 0x04) => "Dialog_ReplyChosen",
            (0x14, 0x05) => "Dialog_Close",
            // Sound-object family verified from EE packet names and
            // `CNWSMessage::SendServerToPlayerSoundObject_*` senders. The
            // semantic translators own exact per-minor CNW cursor validation
            // before strict mode treats these names as safe.
            (0x17, 0x01) => "Sound_Play3D",
            (0x17, 0x02) => "Sound_Object_Play",
            (0x17, 0x03) => "Sound_Object_Stop",
            (0x17, 0x04) => "Sound_Object_ChangeVolume",
            (0x17, 0x05) => "Sound_Object_ChangePosition",
            (0x17, 0x06) => "Sound_Object_Create",
            (0x17, 0x07) => "Sound_Object_Destroy",
            // Journal family verified from EE `CNWSMessage::SendServerToPlayerJournal*`
            // senders and Diamond's Journal client dispatcher. These payloads are
            // accepted unchanged when their high-level shape is otherwise valid.
            (0x1C, 0x01) => "Journal_AddWorld",
            (0x1C, 0x02) => "Journal_AddWorldStrref",
            (0x1C, 0x03) => "Journal_DeleteWorld",
            (0x1C, 0x04) => "Journal_DeleteWorldStrref",
            (0x1C, 0x05) => "Journal_DeleteWorldAll",
            (0x1C, 0x06) => "Journal_AddQuest",
            (0x1C, 0x07) => "Journal_RemoveQuest",
            (0x1C, 0x08) => "Journal_SetQuestPicture",
            (0x1C, 0x09) => "Journal_FullUpdate",
            (0x1C, 0x0C) => "Journal_Updated",
            // Quickbar family confirmed from EE's packet-name table and
            // `CNWSMessage::SendServerToPlayerGuiQuickbar_SetButton`, which
            // sends family 0x1E with minor 1 for the full bar and minor 2 for
            // a single slot after obtaining a CNWMessage write buffer.
            (0x1E, 0x01) => "GuiQuickbar_SetAllButtons",
            (0x1E, 0x02) => "GuiQuickbar_SetButton",
            // SafeProjectile family confirmed from EE packet-name table and
            // `CNWSMessage::SendServerToPlayerSafeProjectile`; the focused
            // translator validates the typed spawn branch before strict mode
            // treats the packet as safe.
            (0x22, 0x01) => "SafeProjectile_Spawn",
            (0x2C, 0x01) => "LoadBar_Start",
            (0x2C, 0x02) => "LoadBar_Update",
            (0x2C, 0x03) => "LoadBar_End",
            (0x31, 0x01) => "PlayModuleCharacterList_Start",
            (0x31, 0x02) => "PlayModuleCharacterList_Stop",
            (0x31, 0x03) => "PlayModuleCharacterList_Response",
            (0x32, 0x01) => "SetCustomToken",
            (0x32, 0x02) => "SetCustomTokenList",
            // Cutscene family confirmed from EE's packet-name table and the
            // exported `CNWSMessage::SendServerToPlayerCutscene_*` senders.
            // The semantic cutscene translator owns exact per-minor shape
            // validation before strict mode treats these names as safe.
            (0x33, 0x01) => "Cutscene_Status",
            (0x33, 0x02) => "Cutscene_Cancel",
            (0x33, 0x03) => "Cutscene_FadeToBlack",
            (0x33, 0x04) => "Cutscene_FadeFromBlack",
            (0x33, 0x05) => "Cutscene_StopFade",
            (0x33, 0x06) => "Cutscene_BlackScreen",
            (0x33, 0x07) => "Cutscene_HideGui",
            (0x35, 0x01) => "GuiEvent_Notify",
            _ => "<unknown>",
        }
    }

    pub fn is_known(self) -> bool {
        self.name() != "<unknown>"
    }
}
