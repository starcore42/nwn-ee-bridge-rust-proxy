//! Generic legacy 1.69 module-resource profile.
//!
//! This intentionally contains no server-specific HAK knowledge. It is useful
//! for tests and as the future default once the bridge can infer resource
//! lists from legacy module metadata instead of using an HG profile.

use super::ModuleResourceProfile;

pub(crate) fn module_resources_profile() -> ModuleResourceProfile {
    ModuleResourceProfile {
        name: "generic-169",
        hak_order_top_first: &[],
        advertise_nwsync: false,
        discovery_session_name: "NWN 1.69 Server",
        discovery_module_name: "NWN 1.69 Module",
    }
}
