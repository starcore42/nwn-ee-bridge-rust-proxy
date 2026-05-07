//! Server/module profiles for legacy 1.69 compatibility data.
//!
//! Translation code should stay generic where possible. Server-specific facts
//! such as Higher Ground's HAK order live here so they can be swapped or
//! tested independently from the packet writer.

pub(crate) mod generic_169;
pub(crate) mod higher_ground;

#[derive(Debug, Clone, Copy)]
pub(crate) struct ModuleResourceProfile {
    pub(crate) name: &'static str,
    pub(crate) hak_order_top_first: &'static [&'static str],
    pub(crate) advertise_nwsync: bool,
}

pub(crate) fn module_resources_profile(name: &str) -> ModuleResourceProfile {
    match name {
        "generic-169" => generic_169::module_resources_profile(),
        "higher-ground" => higher_ground::module_resources_profile(),
        _ => higher_ground::module_resources_profile(),
    }
}
