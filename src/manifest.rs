use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use sha2::{Digest, Sha256};

/// relative path -> content hash. Lets re-indexing skip unchanged files.
pub type Manifest = HashMap<String, String>;

pub fn file_hash(content: &str) -> String {
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    hex::encode(&h.finalize()[..16])
}

pub fn load(path: &Path) -> Manifest {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, m: &Manifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(m)?)?;
    Ok(())
}
