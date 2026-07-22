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

fn optional_usize(value: Option<usize>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

fn optional_u8(value: Option<u8>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

fn main() -> anyhow::Result<()> {
    let config = Config::parse();

    if config.terminal_writer_trace_preflight {
        let path = config
            .terminal_writer_trace
            .as_deref()
            .expect("clap requires --terminal-writer-trace for preflight");
        if let Some(proof_payload_path) = config.terminal_writer_trace_proof_payload.as_deref() {
            let summary =
                translate::live_object_update::preflight_terminal_writer_trace_proof_paths(
                    path,
                    proof_payload_path,
                )
                .map_err(|reason| {
                    anyhow::anyhow!(
                        "terminal writer trace proof preflight journal={} payload={}: {reason}",
                        path.display(),
                        proof_payload_path.display()
                    )
                })?;
            let ready = summary.ready();
            println!(
                "terminal-writer-trace-proof-preflight\tstatus\t{}\tsnapshot_integrity\twindows-deny-write\tartifact_count\t{}\tdistinct_finalized_payload_count\t{}\tduplicate_payload_group_count\t{}\tmax_payload_match_count\t{}\ttarget_payload_bytes\t{}\tterminal_requirement_observed\t{}\tselection_status\t{}\tpayload_match_count\t{}\twriter_handoff_verdict\t{}\twriter_handoff_observed\t{}\tsource_record_offset\t{}\tsource_read_buffer_cursor\t{}\tsource_read_buffer_end\t{}\tsource_fragment_bit_start\t{}\tsource_fragment_bit_end\t{}\tsource_fragment_bit_count\t{}\temitted_read_buffer_cursor\t{}\temitted_read_buffer_end\t{}\temitted_fragment_bit_start\t{}\temitted_fragment_bit_end\t{}\temitted_fragment_bit_count\t{}\tproof_join_ready\t{}\texact_final_validator_accepted\t{}\tcandidate_payload_bytes\t{}\tcandidate_fragment_bit_end\t{}\tcandidate_fragment_final_bits\t{}\tterminal_exact_writer_rewrites\t{}",
                if ready { "ready" } else { "not-ready" },
                summary.journal.artifact_count,
                summary.journal.distinct_finalized_payload_count,
                summary.journal.duplicate_payload_group_count,
                summary.journal.max_payload_match_count,
                summary.target_payload_bytes,
                summary.terminal_requirement_observed,
                summary.selection_status,
                summary.payload_match_count,
                summary.writer_handoff_verdict,
                summary.writer_handoff_observed,
                optional_usize(summary.source_record_offset),
                optional_usize(summary.source_read_buffer_cursor),
                optional_usize(summary.source_read_buffer_end),
                optional_usize(summary.source_fragment_bit_start),
                optional_usize(summary.source_fragment_bit_end),
                optional_usize(summary.source_fragment_bit_count),
                optional_usize(summary.emitted_read_buffer_cursor),
                optional_usize(summary.emitted_read_buffer_end),
                optional_usize(summary.emitted_fragment_bit_start),
                optional_usize(summary.emitted_fragment_bit_end),
                optional_usize(summary.emitted_fragment_bit_count),
                summary.proof_join_ready,
                summary.exact_final_validator_accepted,
                optional_usize(summary.candidate_payload_bytes),
                optional_usize(summary.candidate_fragment_bit_end),
                optional_u8(summary.candidate_fragment_final_bits),
                summary.terminal_exact_writer_rewrites,
            );
            if !ready {
                anyhow::bail!("terminal writer trace proof preflight is not ready");
            }
            return Ok(());
        }
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
        let (summary, rewrite_scope, proof_summary) = if let Some(proof_payload_path) =
            config.terminal_writer_trace_proof_payload.as_deref()
        {
            let proof = translate::live_object_update::configure_terminal_writer_trace_proof_paths(
                path.clone(),
                proof_payload_path,
            )
            .map_err(|reason| {
                anyhow::anyhow!(
                    "terminal writer runtime proof journal={} payload={}: {reason}",
                    path.display(),
                    proof_payload_path.display()
                )
            })?;
            (proof.journal.clone(), "exact-proof-payload", Some(proof))
        } else {
            let summary =
                translate::live_object_update::configure_terminal_writer_trace_path(path.clone())
                    .map_err(|reason| {
                    anyhow::anyhow!("terminal writer trace {}: {reason}", path.display())
                })?;
            (summary, "diagnostic-only", None)
        };
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
            rewrite_scope,
            authorized_proof_payload_path = ?config.terminal_writer_trace_proof_payload,
            authorized_target_payload_bytes = ?proof_summary.as_ref().map(|proof| proof.target_payload_bytes),
            authorized_source_record_offset = ?proof_summary.as_ref().and_then(|proof| proof.source_record_offset),
            authorized_source_fragment_bit_start = ?proof_summary.as_ref().and_then(|proof| proof.source_fragment_bit_start),
            authorized_source_fragment_bit_end = ?proof_summary.as_ref().and_then(|proof| proof.source_fragment_bit_end),
            authorized_emitted_fragment_bit_start = ?proof_summary.as_ref().and_then(|proof| proof.emitted_fragment_bit_start),
            authorized_emitted_fragment_bit_end = ?proof_summary.as_ref().and_then(|proof| proof.emitted_fragment_bit_end),
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
