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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_hash_is_deterministic_and_fixed_width() {
        let a = file_hash("hello world");
        assert_eq!(a, file_hash("hello world"));
        assert_ne!(a, file_hash("hello world!"));
        assert_eq!(a.len(), 32); // 16 bytes -> 32 hex chars
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn load_missing_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let m = load(&dir.path().join("nope.json"));
        assert!(m.is_empty());
    }

    #[test]
    fn save_then_load_roundtrips_and_creates_parent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".rebase-index").join("manifest.json");
        let mut m = Manifest::new();
        m.insert("src/main.rs".into(), "deadbeef".into());
        m.insert("a'b.rs".into(), "cafef00d".into());
        save(&path, &m).unwrap();
        assert_eq!(load(&path), m);
    }
}
