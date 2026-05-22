mod config;
mod crc;
mod ee_crypto;
mod identity;
mod log;
mod net;
mod nwsync;
mod packet;
mod strict;
mod translate;

#[cfg(test)]
mod architecture_guard;

use anyhow::Context;
use clap::Parser;
use config::Config;

fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    log::init(&config).context("initializing logging")?;

    if config.packet_dump {
        if let Some(log_path) = &config.log {
            if let Some(parent) = log_path.parent() {
                let quarantine_dir = parent.join("quarantine");
                translate::diagnostics::set_default_diagnostic_dump_dir(quarantine_dir.clone());
                tracing::info!(
                    path = %quarantine_dir.display(),
                    "default quarantine dump directory enabled from proxy log path"
                );
            }
        }
    }

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        listen = %config.listen,
        server = %config.server,
        strict_translate = config.strict_translate,
        strict_profile = config.strict_profile.as_str(),
        asset_profile = %config.asset_profile,
        synthetic_area_loadbar = config.synthetic_area_loadbar_enabled(),
        allow_remote_clients = config.allow_remote_clients,
        "hgbridge_proxy2 starting"
    );

    let nwsync_runtime = nwsync::Runtime::load(&config).context("loading NWSync runtime config")?;
    if let Some(runtime) = &nwsync_runtime {
        tracing::info!(
            root = ?runtime.root(),
            root_hash = %runtime.advertisement().root_hash(),
            url = %runtime.advertisement().url(),
            "NWSync runtime advertisement enabled"
        );
    }

    net::run(config, nwsync_runtime)
}
