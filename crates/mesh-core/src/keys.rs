//! Provider keys, stored on this machine only in keys.env (mode 600).
use std::collections::BTreeMap;
use std::io::Write;

use crate::config::config;

/// Every key the app knows about, in the order the Keys dialog shows them.
pub const KNOWN: &[&str] = &["FAL_KEY", "MESHY_API_KEY", "HF_TOKEN", "SKETCHFAB_TOKEN", "POLYPIZZA_KEY"];

/// Read KEY=value lines from the keys file; real environment variables win.
pub fn load() -> BTreeMap<String, String> {
    let mut keys = BTreeMap::new();
    if let Ok(text) = std::fs::read_to_string(&config().keys_file) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                keys.insert(k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string());
            }
        }
    }
    for k in KNOWN {
        if let Ok(v) = std::env::var(k)
            && !v.is_empty()
        {
            keys.insert(k.to_string(), v);
        }
    }
    keys
}

pub fn get(name: &str) -> Option<String> {
    load().remove(name).filter(|v| !v.is_empty())
}

/// Merge non-empty values into keys.env. Returns the names now saved.
pub fn save(updates: &BTreeMap<String, String>) -> std::io::Result<Vec<String>> {
    let path = &config().keys_file;
    let mut keys = load();
    for (k, v) in updates {
        if KNOWN.contains(&k.as_str()) && !v.trim().is_empty() {
            keys.insert(k.clone(), v.trim().to_string());
        }
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    for (k, v) in &keys {
        writeln!(f, "{k}={v}")?;
    }
    Ok(keys.into_keys().collect())
}
