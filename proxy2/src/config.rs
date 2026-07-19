use std::{net::SocketAddr, path::PathBuf, str::FromStr, time::Duration};

use anyhow::{Context, anyhow};
use clap::{Parser, ValueEnum};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StrictProfile {
    /// Development profile. Packet families still require exact validators;
    /// this profile only changes diagnostic tolerance outside strict shape
    /// ownership.
    Developer,
    /// Alpha profile. This is the default while the bridge is still under
    /// construction, but high-level M payloads still require exact validators.
    Alpha,
    /// Player-ready profile: exact validators only, with no broad wrapper
    /// allowance.
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum NwsyncAdvertiseMode {
    /// Advertise NWSync in both the direct BNXR pre-connect response and the
    /// EE ServerStatus_ModuleRunning module-resource packet.
    Both,
    /// Advertise only in the EE module-resource packet. This avoids EE's native
    /// pre-connect BNDM downloader handoff while still mounting assets through
    /// the decompile-backed `CNWCModule::LoadModuleResources` reader.
    ModuleOnly,
    /// Advertise only in BNXR. Useful for isolating EE's native downloader path.
    BnxrOnly,
    /// Serve any configured repository but do not advertise it in translated
    /// packets. `--disable-nwsync` still disables both advertisement and HTTP.
    Off,
}

impl NwsyncAdvertiseMode {
    pub fn advertises_bnxr(self) -> bool {
        matches!(self, Self::Both | Self::BnxrOnly)
    }

    pub fn advertises_module_resources(self) -> bool {
        matches!(self, Self::Both | Self::ModuleOnly)
    }
}

#[derive(Debug, Clone, Parser)]
#[command(name = "hgbridge_proxy2")]
#[command(about = "Structured strict-translation NWN EE <-> 1.69 bridge proxy")]
pub struct Config {
    #[arg(long, default_value = "0.0.0.0:5121")]
    pub listen: SocketAddr,

    #[arg(long, value_parser = parse_server, default_value = "111")]
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

    /// Optional JSON file updated with the current pending quickbar item-refresh
    /// candidate. Harness drivers can poll this to target the exact object id
    /// proven by verified inventory/live-object state after a committed
    /// `GuiQuickbar_SetAllButtons`.
    #[arg(long)]
    pub quickbar_item_refresh_hint: Option<PathBuf>,

    /// Private operator-side terminal writer trace artifact to correlate with
    /// quarantined `P/05/01` live-object evidence. Requires `--packet-dump`
    /// with `--log`, or `NWN_BRIDGE_QUARANTINE_DIR`, for correlation output.
    ///
    /// Source-writer ownership alone never authorizes mutation. One unique
    /// full-payload v2 match may enable only the terminal rewrite candidate
    /// independently sealed by the typed EE reader and exact final validator;
    /// every incomplete or mismatched proof remains diagnostic-only.
    #[arg(long, value_name = "PATH")]
    pub terminal_writer_trace: Option<PathBuf>,

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

    /// Select which decompile-backed EE packet path advertises NWSync.
    #[arg(long, value_enum, default_value_t = NwsyncAdvertiseMode::Both)]
    pub nwsync_advertise_mode: NwsyncAdvertiseMode,

    /// Harness-only escape hatch for seeded-cache tests.
    ///
    /// EE `CNWCModule::LoadModuleResources` may mount explicit non-root
    /// manifests from the module-resource packet only when `CExoResMan`
    /// already knows those manifests.  Normal player traffic must prove that
    /// with BNXR preflight/download.  Driver harnesses can instead seed
    /// `nwsyncmeta.sqlite3` from the same repository before launch; this flag
    /// documents that precondition and permits `--nwsync-advertise-mode
    /// module-only` with explicit module manifests.
    #[arg(long)]
    pub nwsync_allow_seeded_module_manifests_without_bnxr: bool,

    /// Explicit local bind address for the built-in NWSync HTTP server.
    #[arg(long)]
    pub nwsync_http_bind: Option<SocketAddr>,

    /// Compatibility switch retained for explicitness: synthesize proxy-owned
    /// LoadBar Start/End frames after an audited `Area_ClientArea` rewrite.
    ///
    /// EE decompiles show LoadBar as the server-owned stall-event UI family
    /// (`0x2C`) used by module load (`LoadBar_Start` during
    /// `CServerExoAppInternal::LoadModule`, then `LoadBar_End` before
    /// `ServerStatus_Status`). Local Diamond bridge capture
    /// `local-diamond-bridge-20260517-161322` confirmed that a verified
    /// proxy-owned LoadBar pair lets EE leave the loading screen after the
    /// rewritten `Area_ClientArea`, while the no-loadbar path stayed on
    /// `Loading Area... Sunshine_Vill`.
    ///
    /// The flag is accepted for old scripts and diagnostics; LoadBar synthesis
    /// is enabled by default unless `--no-synthetic-area-loadbar` is supplied.
    #[arg(long)]
    pub synthetic_area_loadbar: bool,

    /// Legacy diagnostic isolation switch retained for old harness scripts.
    ///
    /// If both flags are supplied, this disabling flag wins.
    #[arg(long)]
    pub no_synthetic_area_loadbar: bool,
}

impl Config {
    pub fn session_timeout(&self) -> Duration {
        Duration::from_millis(self.session_timeout_ms)
    }

    pub fn synthetic_area_loadbar_enabled(&self) -> bool {
        !self.no_synthetic_area_loadbar || self.synthetic_area_loadbar
    }
}

fn parse_server(value: &str) -> anyhow::Result<SocketAddr> {
    match value {
        "111" => "158.69.144.21:5121"
            .parse()
            .context("parsing HG server 111 address"),
        "213" => "158.69.144.21:5133"
            .parse()
            .context("parsing HG server 213 address"),
        _ => SocketAddr::from_str(value)
            .with_context(|| format!("parsing --server value '{value}'"))
            .map_err(Into::into),
    }
    .map_err(|err| anyhow!("{err}"))
}
