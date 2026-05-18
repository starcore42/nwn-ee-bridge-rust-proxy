//! Client-originated raw server-admin reliable payloads.
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

#[derive(Debug, Clone, Copy)]
pub struct ClientServerAdminClaimSummary {
    pub packet_name: &'static str,
    pub command: ClientServerAdminCommand,
}

const MODULE_RUN_PAYLOAD: &[u8] = b"sModule.Run";

pub fn claim_payload_if_verified(payload: &[u8]) -> Option<ClientServerAdminClaimSummary> {
    if payload == MODULE_RUN_PAYLOAD {
        return Some(ClientServerAdminClaimSummary {
            packet_name: "ServerAdmin_ModuleRun",
            command: ClientServerAdminCommand::ModuleRun,
        });
    }

    None
}

pub fn raw_payload_shape_valid(payload: &[u8]) -> bool {
    claim_payload_if_verified(payload).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_run_raw_payload_is_claimed_exactly() {
        let claim = claim_payload_if_verified(b"sModule.Run").expect("module run should claim");

        assert_eq!(claim.packet_name, "ServerAdmin_ModuleRun");
        assert_eq!(claim.command, ClientServerAdminCommand::ModuleRun);
    }

    #[test]
    fn uppercase_server_status_and_unknown_admin_commands_are_not_claimed() {
        assert!(claim_payload_if_verified(b"SModule.Run").is_none());
        assert!(claim_payload_if_verified(b"sModule.Load mod").is_none());
        assert!(claim_payload_if_verified(b"sServer.Status").is_none());
    }
}
