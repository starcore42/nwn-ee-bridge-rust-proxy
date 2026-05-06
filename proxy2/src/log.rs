use std::{fs, path::Path};

use anyhow::Context;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;

pub fn init(config: &Config) -> anyhow::Result<Option<WorkerGuard>> {
    let stdout_layer = fmt::layer()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_ansi(false);

    if let Some(path) = &config.log {
        ensure_parent(path)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("opening log file {}", path.display()))?;
        let (writer, guard) = tracing_appender::non_blocking(file);
        let file_layer = fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .with_writer(writer);
        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .init();
        Ok(Some(guard))
    } else {
        tracing_subscriber::registry().with(stdout_layer).init();
        Ok(None)
    }
}

fn ensure_parent(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating log directory {}", parent.display()))?;
        }
    }
    Ok(())
}
