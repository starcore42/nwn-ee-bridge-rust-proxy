use std::{net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, anyhow};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StrictProfile {
    /// Development profile: exact validators and shallow validators are allowed,
    /// with every shallow allowance logged so missing parsers stay visible.
    Developer,
    /// Alpha profile: exact validators are required for critical gameplay
    /// families; shallow validators remain allowed only for non-critical
    /// control/status families while the bridge is still under construction.
    Alpha,
    /// Player-ready profile: exact validators only. Any shallow validator is a
    /// quarantine decision even when its wrapper shape is otherwise plausible.
    Player,
}

impl StrictProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            StrictProfile::Developer => "developer",
            StrictProfile::Alpha => "alpha",
            StrictProfile::Player => "player",
        }
    }

    pub fn allows_shallow_high_level_validator(self, critical: bool) -> bool {
        match self {
            StrictProfile::Developer => true,
            StrictProfile::Alpha => !critical,
            StrictProfile::Player => false,
        }
    }
}

impl std::fmt::Display for StrictProfile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Default for StrictProfile {
    fn default() -> Self {
        StrictProfile::Alpha
    }
}

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

    #[arg(long, value_enum, default_value_t = StrictProfile::Alpha)]
    pub strict_profile: StrictProfile,

    #[arg(long, default_value = "higher-ground")]
    pub asset_profile: String,

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

    /// Optional env file written by the asset/NWSync build pipeline.
    #[arg(long)]
    pub nwsync_env: Option<PathBuf>,

    /// Local NWSync repository root to serve. Overrides env-file root.
    #[arg(long)]
    pub nwsync_root: Option<PathBuf>,

    /// NWSync root manifest hash to advertise. Overrides env-file hash.
    #[arg(long)]
    pub nwsync_hash: Option<String>,

    /// Public NWSync URL to advertise. Overrides env-file URL.
    #[arg(long)]
    pub nwsync_url: Option<String>,

    /// Disable NWSync advertisement and local repository serving.
    #[arg(long)]
    pub disable_nwsync: bool,

    /// Explicit local bind address for the built-in NWSync HTTP server.
    #[arg(long)]
    pub nwsync_http_bind: Option<SocketAddr>,
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
