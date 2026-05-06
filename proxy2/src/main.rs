mod config;
mod crc;
mod ee_crypto;
mod identity;
mod log;
mod net;
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
        allow_remote_clients = config.allow_remote_clients,
        "hgbridge_proxy2 starting"
    );

    net::run(config)
}
