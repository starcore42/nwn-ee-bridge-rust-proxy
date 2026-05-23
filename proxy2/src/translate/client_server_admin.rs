//! Client-originated server-admin reliable payloads.
//!
//! These messages are deliberately not CNW high-level `P major minor`
//! gameplay packets. They are lower-case `s...` text commands carried as the
//! primary reliable `M` payload, then routed by the server-admin handler.
//!
//! Decompile anchors:
//!
//! - Diamond client `sub_438420` builds module-admin requests with
//!   `"%c%s.%s"`, first byte `0x73` (`'s'`), and `"Module"` as the command
//!   namespace. The local Diamond harness capture produced the exact payload
//!   `sModule.Run` after character selection.
//! - EE server `CNWSMessage::HandleServerAdminToServerMessage(uint,uchar*,uint)`
//!   first checks byte `0x73`, strips that byte, tokenizes the remaining
//!   `Namespace.Command` text, and dispatches module/server-admin handlers.
//!
//! The bridge treats this as an explicit semantic no-op family. It is not a
//! transport continuation and must never be accepted by the generic
//! transport-identity layer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientServerAdminCommand {
    ModuleRun,
}

impl ClientServerAdminCommand {
    fn packet_name(self) -> &'static str {
        match self {
            Self::ModuleRun => "ServerAdmin_ModuleRun",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientServerAdminMessage {
    pub command: ClientServerAdminCommand,
}

#[derive(Debug, Clone, Copy)]
pub struct ClientServerAdminClaimSummary {
    pub packet_name: &'static str,
    pub command: ClientServerAdminCommand,
}

const MODULE_RUN_PAYLOAD: &[u8] = b"sModule.Run";

impl ClientServerAdminMessage {
    pub fn parse_exact(payload: &[u8]) -> Option<Self> {
        if payload == MODULE_RUN_PAYLOAD {
            return Some(Self {
                command: ClientServerAdminCommand::ModuleRun,
            });
        }

        None
    }

    pub fn write_legacy(self, out: &mut Vec<u8>) {
        match self.command {
            ClientServerAdminCommand::ModuleRun => out.extend_from_slice(MODULE_RUN_PAYLOAD),
        }
    }

    pub fn to_legacy_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(MODULE_RUN_PAYLOAD.len());
        self.write_legacy(&mut out);
        out
    }

    pub fn validates_exact(self, payload: &[u8]) -> bool {
        self.to_legacy_bytes() == payload
    }
}

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientServerAdminClaimSummary> {
    let message = ClientServerAdminMessage::parse_exact(payload)?;
    if !message.validates_exact(payload) {
        return None;
    }

    Some(ClientServerAdminClaimSummary {
        packet_name: message.command.packet_name(),
        command: message.command,
    })
}

pub fn exact_payload_valid(payload: &[u8]) -> bool {
    claim_payload_if_verified(payload).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_run_payload_is_claimed_exactly() {
        let claim = claim_payload_if_verified(b"sModule.Run").expect("module run should claim");

        assert_eq!(claim.packet_name, "ServerAdmin_ModuleRun");
        assert_eq!(claim.command, ClientServerAdminCommand::ModuleRun);
    }

    #[test]
    fn module_run_round_trips_through_typed_writer() {
        let message =
            ClientServerAdminMessage::parse_exact(b"sModule.Run").expect("module run should parse");

        assert_eq!(message.command, ClientServerAdminCommand::ModuleRun);
        assert_eq!(message.to_legacy_bytes(), b"sModule.Run");
        assert!(message.validates_exact(b"sModule.Run"));
    }

    #[test]
    fn uppercase_server_status_and_unknown_admin_commands_are_not_claimed() {
        assert!(claim_payload_if_verified(b"SModule.Run").is_none());
        assert!(claim_payload_if_verified(b"sModule.Load mod").is_none());
        assert!(claim_payload_if_verified(b"sServer.Status").is_none());
        assert!(!exact_payload_valid(b"sModule.Run\x00"));
    }
}
