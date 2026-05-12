//! Runtime NWSync advertisement and static repository serving.
//!
//! Asset builds are intentionally outside the proxy. The proxy consumes the
//! immutable output of `tools/build-asset-profile.ps1`: a repository root, root
//! hash, and public URL. Packet translators can then advertise exactly that
//! repository without learning anything about HG-specific staging details.

use std::{
    collections::HashMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
    thread,
};

use anyhow::{Context, anyhow};

use crate::config::Config;

const DEFAULT_ENV_PATH: &str = "hg-bridge-nwsync.env";
const ENV_ROOT: &str = "HG_BRIDGE_NWSYNC_ROOT";
const ENV_HASH: &str = "HG_BRIDGE_NWSYNC_HASH";
const ENV_URL: &str = "HG_BRIDGE_NWSYNC_URL";
const ENV_MANIFESTS: &str = "HG_BRIDGE_NWSYNC_MANIFESTS";
const ENV_MODULE_MANIFESTS: &str = "HG_BRIDGE_NWSYNC_MODULE_MANIFESTS";

#[derive(Debug, Clone)]
pub struct ManifestAdvert {
    pub hash: String,
    pub flags: u8,
    pub language: u8,
}

#[derive(Debug, Clone)]
pub struct Advertisement {
    root_hash: String,
    url: String,
    bnxr_manifests: Vec<ManifestAdvert>,
    module_manifests: Vec<ManifestAdvert>,
}

impl Advertisement {
    pub fn new(
        root_hash: String,
        url: String,
        bnxr_manifests: Vec<ManifestAdvert>,
        module_manifests: Vec<ManifestAdvert>,
    ) -> anyhow::Result<Self> {
        validate_counted_ascii("NWSync root hash", &root_hash)?;
        validate_counted_ascii("NWSync URL", &url)?;
        if bnxr_manifests.len() > u8::MAX as usize {
            return Err(anyhow!(
                "too many BNXR NWSync manifest adverts: {}",
                bnxr_manifests.len()
            ));
        }
        if module_manifests.len() > u8::MAX as usize {
            return Err(anyhow!(
                "too many module-resource NWSync manifest adverts: {}",
                module_manifests.len()
            ));
        }
        for manifest in bnxr_manifests.iter().chain(module_manifests.iter()) {
            validate_counted_ascii("NWSync manifest hash", &manifest.hash)?;
        }
        Ok(Self {
            root_hash,
            url,
            bnxr_manifests,
            module_manifests,
        })
    }

    pub fn root_hash(&self) -> &str {
        &self.root_hash
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn manifests(&self) -> &[ManifestAdvert] {
        &self.bnxr_manifests
    }

    pub fn module_manifests(&self) -> &[ManifestAdvert] {
        &self.module_manifests
    }

    pub fn build_bnxr_section(&self) -> anyhow::Result<Vec<u8>> {
        let mut section = Vec::new();
        section.push(0x02);
        section.push(1);
        append_counted(&mut section, self.url())?;
        append_counted(&mut section, self.root_hash())?;
        section
            .push(u8::try_from(self.bnxr_manifests.len()).context("manifest count overflow")?);
        for manifest in &self.bnxr_manifests {
            section.push(manifest.flags);
            section.push(manifest.language);
            append_counted(&mut section, &manifest.hash)?;
        }
        Ok(section)
    }
}

#[derive(Debug, Clone)]
pub struct Runtime {
    root: Option<PathBuf>,
    advertisement: Advertisement,
}

impl Runtime {
    pub fn load(config: &Config) -> anyhow::Result<Option<Self>> {
        if config.disable_nwsync {
            return Ok(None);
        }

        let mut env_values = HashMap::new();
        let env_path = config.nwsync_env.clone().or_else(default_env_path);
        if let Some(path) = env_path {
            env_values = read_env_file(&path)
                .with_context(|| format!("reading NWSync env file {}", path.display()))?;
        }

        let root = config
            .nwsync_root
            .clone()
            .or_else(|| env_values.get(ENV_ROOT).map(PathBuf::from));
        let root_hash = config
            .nwsync_hash
            .clone()
            .or_else(|| env_values.get(ENV_HASH).cloned());
        let url = config
            .nwsync_url
            .clone()
            .or_else(|| env_values.get(ENV_URL).cloned());

        let Some(root_hash) = root_hash.filter(|value| !value.trim().is_empty()) else {
            return Ok(None);
        };
        let Some(url) = url.filter(|value| !value.trim().is_empty()) else {
            return Ok(None);
        };

        let root_hash = root_hash.trim().to_string();
        let bnxr_manifests = parse_manifest_adverts(env_values.get(ENV_MANIFESTS), &root_hash, true)?;
        let module_manifests =
            parse_manifest_adverts(env_values.get(ENV_MODULE_MANIFESTS), &root_hash, false)?;
        if !module_manifests.is_empty() && !config.nwsync_advertise_mode.advertises_bnxr() {
            tracing::warn!(
                module_manifests = module_manifests.len(),
                "NWSync module-resource extra manifests are configured without BNXR preflight advertisement; a cold EE cache may fail CNWCModule::LoadModuleResources before those manifests are locally known"
            );
        }
        let advertisement = Advertisement::new(
            root_hash,
            url.trim().to_string(),
            bnxr_manifests,
            module_manifests,
        )?;
        Ok(Some(Self {
            root,
            advertisement,
        }))
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn advertisement(&self) -> &Advertisement {
        &self.advertisement
    }
}

fn default_env_path() -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    let candidates = [
        cwd.join(DEFAULT_ENV_PATH),
        cwd.parent().map(|parent| parent.join(DEFAULT_ENV_PATH))?,
    ];
    candidates.into_iter().find(|candidate| candidate.is_file())
}

fn parse_manifest_adverts(
    raw: Option<&String>,
    root_hash: &str,
    default_to_root: bool,
) -> anyhow::Result<Vec<ManifestAdvert>> {
    let Some(raw) = raw.map(String::as_str).map(str::trim).filter(|value| !value.is_empty()) else {
        // EE's byte-oriented BNXR pre-module path needs a concrete client
        // content work item, so its default advert list is the root manifest.
        // `CNWCModule::LoadModuleResources` is different: the decompile shows
        // it calls AddManifest(root) first and then treats each explicit
        // manifest advert as another resource key table to mount. Missing
        // explicit manifests can be fatal, so module-resource extras must be
        // opt-in only.
        return if default_to_root {
            Ok(vec![ManifestAdvert {
                hash: root_hash.to_string(),
                flags: 1,
                language: 0xFF,
            }])
        } else {
            Ok(Vec::new())
        };
    };

    let mut manifests = Vec::new();
    for (index, entry) in raw.split([',', ';']).enumerate() {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        let mut fields = entry.split(':').map(str::trim);
        let hash = fields
            .next()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("empty NWSync manifest hash at entry {}", index + 1))?;
        let flags = match fields.next().filter(|value| !value.is_empty()) {
            Some(value) => parse_u8_field("NWSync manifest flags", value)?,
            None => 1,
        };
        let language = match fields.next().filter(|value| !value.is_empty()) {
            Some(value) => parse_u8_field("NWSync manifest language", value)?,
            None => 0xFF,
        };
        if fields.next().is_some() {
            return Err(anyhow!(
                "too many fields in NWSync manifest advert entry {}",
                index + 1
            ));
        }
        manifests.push(ManifestAdvert {
            hash: hash.to_string(),
            flags,
            language,
        });
    }

    Ok(manifests)
}

fn parse_u8_field(label: &str, value: &str) -> anyhow::Result<u8> {
    let parsed = if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u8::from_str_radix(hex, 16)
    } else {
        value.parse::<u8>()
    };
    parsed.with_context(|| format!("parsing {label} value '{value}'"))
}

pub struct HttpServerGuard {
    _handle: thread::JoinHandle<()>,
}

pub fn start_http_server_if_needed(
    config: &Config,
    runtime: Option<&Runtime>,
) -> anyhow::Result<Option<HttpServerGuard>> {
    let Some(runtime) = runtime else {
        return Ok(None);
    };
    let Some(root) = runtime.root().map(Path::to_path_buf) else {
        tracing::info!(
            "NWSync advertisement enabled without a local repository root; no HTTP server started"
        );
        return Ok(None);
    };
    if !root.is_dir() {
        return Err(anyhow!(
            "NWSync repository root does not exist: {}",
            root.display()
        ));
    }

    let Some(bind) = config
        .nwsync_http_bind
        .or_else(|| local_bind_from_url(runtime.advertisement().url()))
    else {
        tracing::info!(
            url = runtime.advertisement().url(),
            "NWSync URL is not local; no HTTP server started"
        );
        return Ok(None);
    };

    let listener =
        TcpListener::bind(bind).with_context(|| format!("binding NWSync HTTP server on {bind}"))?;
    let advertised_url = runtime.advertisement().url().to_string();
    let advertised_hash = runtime.advertisement().root_hash().to_string();
    let handle = thread::spawn(move || {
        tracing::info!(
            bind = %bind,
            root = %root.display(),
            url = %advertised_url,
            root_hash = %advertised_hash,
            "NWSync static repository server started"
        );
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => handle_http_client(stream, &root),
                Err(err) => tracing::warn!(error = %err, "NWSync HTTP accept failed"),
            }
        }
    });
    Ok(Some(HttpServerGuard { _handle: handle }))
}

fn read_env_file(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let mut values = HashMap::new();
    let text = fs::read_to_string(path)?;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        values.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok(values)
}

fn validate_counted_ascii(label: &str, value: &str) -> anyhow::Result<()> {
    if value.len() > u8::MAX as usize {
        return Err(anyhow!(
            "{label} is too long for counted BN/NWSync encoding"
        ));
    }
    if !value.is_ascii() {
        return Err(anyhow!("{label} must be ASCII for BN/NWSync advertisement"));
    }
    Ok(())
}

fn append_counted(out: &mut Vec<u8>, value: &str) -> anyhow::Result<()> {
    validate_counted_ascii("counted NWSync string", value)?;
    out.push(u8::try_from(value.len()).context("counted string overflow")?);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn local_bind_from_url(url: &str) -> Option<SocketAddr> {
    let rest = url.strip_prefix("http://")?;
    let host_port = rest.split('/').next().unwrap_or(rest);
    let (host, port) = host_port
        .rsplit_once(':')
        .map(|(host, port)| (host, port.parse::<u16>().ok()))
        .unwrap_or((host_port, Some(80)));
    let port = port?;
    match host {
        "127.0.0.1" | "localhost" => Some(SocketAddr::from(([127, 0, 0, 1], port))),
        _ => None,
    }
}

fn handle_http_client(mut stream: TcpStream, root: &Path) {
    if let Err(err) = try_handle_http_client(&mut stream, root) {
        tracing::warn!(error = %err, "NWSync HTTP request failed");
        let _ = write_response(&mut stream, 500, "Internal Server Error", b"", false);
    }
}

fn try_handle_http_client(stream: &mut TcpStream, root: &Path) -> anyhow::Result<()> {
    let mut request = [0u8; 4096];
    let len = stream.read(&mut request)?;
    let text = String::from_utf8_lossy(&request[..len]);
    let Some(first_line) = text.lines().next() else {
        tracing::warn!(status = 400u16, "NWSync HTTP malformed empty request");
        write_response(stream, 400, "Bad Request", b"", false)?;
        return Ok(());
    };
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let raw_path = parts.next().unwrap_or_default();
    if method != "GET" && method != "HEAD" {
        tracing::warn!(
            method,
            raw_path,
            status = 405u16,
            "NWSync HTTP rejected unsupported method"
        );
        write_response(stream, 405, "Method Not Allowed", b"", false)?;
        return Ok(());
    }

    let Some(relative) = sanitize_http_path(raw_path) else {
        tracing::warn!(
            method,
            raw_path,
            status = 400u16,
            "NWSync HTTP rejected unsafe path"
        );
        write_response(stream, 400, "Bad Request", b"", false)?;
        return Ok(());
    };
    let path = root.join(relative);
    if !path.is_file() {
        tracing::warn!(
            method,
            raw_path,
            resolved = %path.display(),
            status = 404u16,
            "NWSync HTTP missing repository object"
        );
        write_response(stream, 404, "Not Found", b"", false)?;
        return Ok(());
    }
    let body = fs::read(&path)?;
    tracing::info!(
        method,
        raw_path,
        resolved = %path.display(),
        status = 200u16,
        bytes = body.len(),
        headers_only = method == "HEAD",
        "NWSync HTTP served repository object"
    );
    write_response(stream, 200, "OK", &body, method == "HEAD")?;
    Ok(())
}

fn sanitize_http_path(raw_path: &str) -> Option<PathBuf> {
    let path_without_query = raw_path.split('?').next().unwrap_or(raw_path);
    if path_without_query.contains('%') || path_without_query.contains('\\') {
        return None;
    }
    let trimmed = path_without_query.trim_start_matches('/');
    let relative = if trimmed.is_empty() {
        "latest"
    } else {
        trimmed
    };
    let candidate = Path::new(relative);
    let mut clean = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            _ => return None,
        }
    }
    Some(clean)
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    body: &[u8],
    headers_only: bool,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/octet-stream\r\n\r\n",
        body.len()
    )?;
    if !headers_only {
        stream.write_all(body)?;
    }
    Ok(())
}
