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

use anyhow::Context;
use clap::Parser;
use config::Config;

fn main() -> anyhow::Result<()> {
    let config = Config::parse();
    let _log_guard = log::init(&config).context("initializing logging")?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        listen = %config.listen,
        server = %config.server,
        strict_translate = config.strict_translate,
        strict_profile = config.strict_profile.as_str(),
        asset_profile = %config.asset_profile,
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
