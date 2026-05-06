use std::{net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, anyhow};
use clap::Parser;

#[derive(Debug, Clone, Parser)]
#[command(name = "hgbridge_proxy2")]
#[command(about = "Structured strict-translation NWN EE <-> 1.69 bridge proxy")]
pub struct Config {
    #[arg(long, default_value = "0.0.0.0:5121")]
    pub listen: SocketAddr,

    #[arg(long, value_parser = parse_server, default_value = "213")]
    pub server: SocketAddr,

    #[arg(long)]
    pub log: Option<PathBuf>,

    #[arg(long)]
    pub allow_remote_clients: bool,

    #[arg(long, alias = "strict-translation", alias = "no-passthrough")]
    pub strict_translate: bool,

    #[arg(long, default_value_t = 300_000)]
    pub session_timeout_ms: u64,

    #[arg(long)]
    pub packet_dump: bool,

    /// Diamond `nwncdkey.ini` used to derive 1.69 public CD-key fields.
    ///
    /// The Starcore5 harness account defaults to `C:\NWN\Config\5.nwncdkey.ini`
    /// when this is not supplied, matching the current test workflow.
    #[arg(long)]
    pub diamond_cdkey: Option<PathBuf>,

    /// Diamond/1.69 private build value written into translated `BNCS`.
    #[arg(long, default_value_t = 8109)]
    pub bncs_private_build: u32,

    /// Diamond/1.69 auth/build field written into translated `BNCS`.
    #[arg(long, default_value_t = 0x0003)]
    pub bncs_build_field: u16,
}

impl Config {
    pub fn session_timeout(&self) -> Duration {
        Duration::from_millis(self.session_timeout_ms)
    }
}

fn parse_server(value: &str) -> anyhow::Result<SocketAddr> {
    match value {
        "213" => "158.69.144.21:5133"
            .parse()
            .context("parsing HG server 213 address"),
        _ => SocketAddr::from_str(value)
            .with_context(|| format!("parsing --server value '{value}'"))
            .map_err(Into::into),
    }
    .map_err(|err| anyhow!("{err}"))
}
