//! Higher Ground module-resource profile.
//!
//! These HAK resrefs are HG-specific resource knowledge. Keeping them out of
//! `module_resources.rs` preserves a generic EE module-resource writer and
//! makes future non-HG profiles possible without touching packet code.

use super::ModuleResourceProfile;

// Fallback-only copy of the currently observed HG `Module_Info` HAK stack.
// Runtime packet translation records the server-provided list from
// `Module_Info` and does not synthesize from this profile. Keep this fallback
// conservative so offline resource-table probes do not mount HAKs the server
// did not declare.
const HAK_ORDER_TOP_FIRST: [&str; 23] = [
    "cep2_custom",
    "cep2_top_v23",
    "cep2_add_phenos5",
    "cep2_add_phenos4",
    "cep2_add_phenos3",
    "cep2_add_phenos2",
    "cep2_add_phenos1",
    "cep2_add_loads",
    "cep2_add_rules",
    "cep2_add_sb_v1",
    "cep2_core6",
    "cep2_core5",
    "cep2_core4",
    "cep2_core3",
    "cep2_core2",
    "cep2_core1",
    "cep2_core0",
    "cep2_add_doors",
    "cep2_add_tiles2",
    "cep2_add_tiles1",
    "cep2_ext_tiles",
    "cep2_add_skies",
    "cep2_crp",
];

pub(crate) fn module_resources_profile() -> ModuleResourceProfile {
    ModuleResourceProfile {
        name: "higher-ground",
        hak_order_top_first: &HAK_ORDER_TOP_FIRST,
        advertise_nwsync: false,
    }
}
