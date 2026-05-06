//! Diamond identity material used by strict legacy-auth translators.
//!
//! The driver-only harness does not patch EE's account identity, so proxy2 must
//! synthesize the 1.69-facing public CD key fields from the configured Diamond
//! account material. This module only loads identity data and derives verifier
//! material; packet rewriting lives in `translate`.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use crate::config::Config;

#[derive(Debug, Clone, Default)]
pub struct DiamondIdentity {
    pub cd_keys: Vec<String>,
    pub public_cd_keys: Vec<String>,
    pub source: Option<PathBuf>,
}

impl DiamondIdentity {
    pub fn load(config: &Config) -> Self {
        let Some(path) = diamond_cdkey_path(config) else {
            tracing::info!("no Diamond CD key source configured for proxy2");
            return Self::default();
        };

        let Ok(contents) = fs::read_to_string(&path) else {
            tracing::warn!(path = %path.display(), "Diamond CD key source unreadable");
            return Self {
                source: Some(path),
                ..Self::default()
            };
        };

        let cd_keys = parse_cdkey_ini_values(&contents);
        let public_cd_keys = cd_keys
            .iter()
            .filter_map(|key| unscramble_cdkey_public_part(key))
            .collect::<Vec<_>>();

        tracing::info!(
            path = %path.display(),
            raw_count = cd_keys.len(),
            public_count = public_cd_keys.len(),
            primary_public = public_cd_keys.first().map(String::as_str).unwrap_or("<none>"),
            "Diamond CD key source loaded for proxy2"
        );

        Self {
            cd_keys,
            public_cd_keys,
            source: Some(path),
        }
    }

    pub fn primary_public_key(&self) -> Option<&str> {
        self.public_cd_keys.first().map(String::as_str)
    }

    pub fn legacy_cdkey_verifiers(&self, challenge: &[u8]) -> anyhow::Result<Vec<String>> {
        if challenge.is_empty() {
            anyhow::bail!("empty CD key verifier challenge");
        }

        let count = self.cd_keys.len().min(self.public_cd_keys.len());
        if count == 0 {
            anyhow::bail!("no usable Diamond CD key verifier material loaded");
        }

        let mut verifiers = Vec::with_capacity(count);
        for index in 0..count {
            verifiers.push(build_legacy_cdkey_verifier(
                &self.cd_keys[index],
                &self.public_cd_keys[index],
                challenge,
            )?);
        }
        Ok(verifiers)
    }
}

pub fn looks_like_public_cdkey(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn diamond_cdkey_path(config: &Config) -> Option<PathBuf> {
    if let Some(path) = &config.diamond_cdkey {
        return Some(path.clone());
    }
    if let Ok(value) = env::var("HG_BRIDGE_DIAMOND_CDKEY_PATH") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    let default_starcore5 = Path::new(r"C:\NWN\Config\5.nwncdkey.ini");
    if default_starcore5.exists() {
        return Some(default_starcore5.to_path_buf());
    }
    None
}

fn parse_cdkey_ini_values(contents: &str) -> Vec<String> {
    contents
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                return None;
            }
            let (_, value) = line.split_once('=')?;
            let key = normalize_cdkey_value(value);
            (key.len() == 41).then_some(key)
        })
        .collect()
}

fn normalize_cdkey_value(value: &str) -> String {
    let mut result = value.trim().to_ascii_uppercase();
    if result.len() >= 2 {
        let first = result.as_bytes()[0];
        let last = result.as_bytes()[result.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            result = result[1..result.len() - 1].to_string();
        }
    }
    result
}

fn build_legacy_cdkey_verifier(
    raw_key: &str,
    public_key: &str,
    challenge: &[u8],
) -> anyhow::Result<String> {
    if raw_key.is_empty() || !looks_like_public_cdkey(public_key) || challenge.is_empty() {
        anyhow::bail!("invalid CD key verifier input");
    }

    let mut digest_input = Vec::with_capacity(raw_key.len() + challenge.len());
    digest_input.extend_from_slice(raw_key.as_bytes());
    digest_input.extend_from_slice(challenge);
    let digest = format!("{:x}", md5::compute(digest_input));
    let verifier = format!("{public_key}{digest}");
    if verifier.len() != 40 {
        anyhow::bail!("invalid CD key verifier length {}", verifier.len());
    }
    Ok(verifier)
}

fn unscramble_cdkey_public_part(key: &str) -> Option<String> {
    if key.len() != 41 {
        return None;
    }

    let mut public_key = String::new();
    let mut private_part = String::new();
    let mut checksum_part = String::new();
    let mut state = 0_u8;
    let mut index = 0;
    let bytes = key.as_bytes();

    while index < bytes.len() {
        let ch = bytes[index] as char;
        let mut reprocess = false;
        if ch != '-' {
            match state {
                0 => {
                    state = 1;
                    if private_part.len() < 20 {
                        private_part.push(ch);
                    }
                }
                1 => {
                    state = 2;
                    if private_part.len() <= 12 {
                        state = 0;
                        if public_key.len() < 8 {
                            public_key.push(ch);
                        } else if private_part.len() < 20 || checksum_part.len() < 7 {
                            reprocess = true;
                        }
                    } else if checksum_part.len() < 7 {
                        checksum_part.push(ch);
                    }
                }
                _ => {
                    state = 0;
                    if public_key.len() < 8 {
                        public_key.push(ch);
                    } else if private_part.len() < 20 || checksum_part.len() < 7 {
                        reprocess = true;
                    }
                }
            }
        }

        if !reprocess {
            index += 1;
        }
    }

    looks_like_public_cdkey(&public_key).then_some(public_key)
}
