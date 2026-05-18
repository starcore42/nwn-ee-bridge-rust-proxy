use std::{
    fs::{self, File},
    io::{self, Write},
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use tracing_subscriber::{fmt, fmt::MakeWriter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::Config;

pub fn init(config: &Config) -> anyhow::Result<()> {
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
        let writer = FlushFileMakeWriter::new(file);
        let file_layer = fmt::layer()
            .with_target(false)
            .with_ansi(false)
            .with_writer(writer);
        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .init();
        Ok(())
    } else {
        tracing_subscriber::registry().with(stdout_layer).init();
        Ok(())
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

#[derive(Clone)]
struct FlushFileMakeWriter {
    file: Arc<Mutex<File>>,
}

impl FlushFileMakeWriter {
    fn new(file: File) -> Self {
        Self {
            file: Arc::new(Mutex::new(file)),
        }
    }
}

impl<'writer> MakeWriter<'writer> for FlushFileMakeWriter {
    type Writer = FlushFileWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        FlushFileWriter {
            file: Arc::clone(&self.file),
        }
    }
}

struct FlushFileWriter {
    file: Arc<Mutex<File>>,
}

impl Write for FlushFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("structured log mutex poisoned"))?;
        file.write_all(buf)?;
        file.flush()?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("structured log mutex poisoned"))?;
        file.flush()
    }
}
