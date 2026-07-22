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

    if config.terminal_writer_trace_preflight {
        let path = config
            .terminal_writer_trace
            .as_deref()
            .expect("clap requires --terminal-writer-trace for preflight");
        let summary = translate::live_object_update::preflight_terminal_writer_trace_path(path)
            .map_err(|reason| {
                anyhow::anyhow!("terminal writer trace {}: {reason}", path.display())
            })?;
        println!(
            "terminal-writer-trace-preflight\tstatus\tloaded\tsnapshot_integrity\twindows-deny-write\tartifact_count\t{}\tfinalized_payload_bytes\t{}\tpayload_length_min\t{}\tpayload_length_max\t{}\tdistinct_finalized_payload_count\t{}\tduplicate_payload_group_count\t{}\tmax_payload_match_count\t{}\ttrace_id_min\t{}\ttrace_id_max\t{}\tdistinct_producer_component_count\t{}\tproducer_reported_component_sha256\t{}",
            summary.artifact_count,
            summary.finalized_payload_bytes,
            summary.payload_length_min,
            summary.payload_length_max,
            summary.distinct_finalized_payload_count,
            summary.duplicate_payload_group_count,
            summary.max_payload_match_count,
            summary.trace_id_min,
            summary.trace_id_max,
            summary.distinct_producer_component_count(),
            summary.producer_reported_component_sha256_csv(),
        );
        return Ok(());
    }

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

    if let Some(path) = config.terminal_writer_trace.clone() {
        let summary =
            translate::live_object_update::configure_terminal_writer_trace_path(path.clone())
                .map_err(|reason| {
                    anyhow::anyhow!("terminal writer trace {}: {reason}", path.display())
                })?;
        let output_dir = translate::diagnostics::probe_dump_dir().ok_or_else(|| {
            anyhow::anyhow!(
                "--terminal-writer-trace requires a diagnostic output: use --packet-dump with --log, or set NWN_BRIDGE_QUARANTINE_DIR"
            )
        })?;
        tracing::info!(
            path = %path.display(),
            output_dir = %output_dir.display(),
            artifact_count = summary.artifact_count,
            finalized_payload_bytes = summary.finalized_payload_bytes,
            payload_length_min = summary.payload_length_min,
            payload_length_max = summary.payload_length_max,
            distinct_finalized_payload_count = summary.distinct_finalized_payload_count,
            duplicate_payload_group_count = summary.duplicate_payload_group_count,
            max_payload_match_count = summary.max_payload_match_count,
            trace_id_min = summary.trace_id_min,
            trace_id_max = summary.trace_id_max,
            distinct_producer_component_count = summary.distinct_producer_component_count(),
            producer_reported_component_sha256 = %summary.producer_reported_component_sha256_csv(),
            "loaded bounded private terminal writer trace journal"
        );
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
