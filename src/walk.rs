use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct FileEntry {
    pub path: PathBuf,
    pub relative: String,
    pub language: String,
}

/// Map a file extension to a language tag. Returns None for files we don't index.
pub fn language_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit('.').next().unwrap_or("").to_lowercase();
    Some(match ext.as_str() {
        "js" | "mjs" | "cjs" => "javascript",
        "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "typescript",
        "vue" => "vue",
        "json" | "jsonc" => "json",
        "css" | "scss" | "less" => "css",
        "html" | "htm" => "html",
        "md" | "markdown" => "markdown",
        "py" => "python",
        "go" => "go",
        "rs" => "rust",
        "php" => "php",
        "rb" => "ruby",
        "java" => "java",
        "ex" | "exs" | "heex" | "eex" => "elixir",
        "erl" | "hrl" => "erlang",
        "c" | "h" => "c",
        "cpp" | "cc" | "hpp" | "cxx" => "cpp",
        "sh" | "bash" => "shell",
        "yml" | "yaml" => "yaml",
        "toml" => "toml",
        "sql" => "sql",
        _ => return None,
    })
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(256).any(|&b| b == 0)
}

/// Walk `root` recursively. Unlike a typical IDE walk this DOES descend into
/// dependency trees (node_modules, vendor, target, …) — indexing library code
/// is the point. Only `.git`, binaries, and oversized files are skipped.
/// `langs` (if non-empty) filters to those language tags.
pub fn walk(root: &Path, langs: &[String], max_bytes: u64) -> Vec<FileEntry> {
    let mut out = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let Some(language) = language_for(&name) else {
            continue;
        };
        if !langs.is_empty() && !langs.iter().any(|l| l == language) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.len() > max_bytes || meta.len() == 0 {
            continue;
        }
        let path = entry.path().to_path_buf();
        let Ok(head) = std::fs::read(&path) else {
            continue;
        };
        if looks_binary(&head) {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(FileEntry {
            path,
            relative,
            language: language.to_string(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;

    #[test]
    fn language_for_maps_known_extensions() {
        assert_eq!(language_for("main.rs"), Some("rust"));
        assert_eq!(language_for("App.tsx"), Some("typescript"));
        assert_eq!(language_for("index.JSX"), Some("javascript")); // case-insensitive
        assert_eq!(language_for("page.vue"), Some("vue"));
        assert_eq!(language_for("mix.exs"), Some("elixir"));
        assert_eq!(language_for("node.erl"), Some("erlang"));
        assert_eq!(language_for("style.scss"), Some("css"));
    }

    #[test]
    fn language_for_rejects_unknown_and_extensionless() {
        assert_eq!(language_for("a.out"), None);
        assert_eq!(language_for("Makefile"), None);
        assert_eq!(language_for("LICENSE"), None);
    }

    #[test]
    fn looks_binary_detects_null_byte() {
        assert!(looks_binary(b"\x7fELF\x00\x00"));
        assert!(!looks_binary(b"fn main() {}\n"));
        // null beyond the 256-byte sniff window is not inspected
        let mut late = vec![b'a'; 300];
        late[280] = 0;
        assert!(!looks_binary(&late));
    }

    #[test]
    fn walk_filters_git_binaries_empty_and_by_language() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("keep.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("notes.md"), "# hi\n").unwrap();
        fs::write(root.join("empty.rs"), "").unwrap();
        fs::write(root.join("bin.rs"), b"abc\x00def").unwrap();
        fs::write(root.join("ignore.unknown"), "x").unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join(".git").join("config.rs"), "fn x() {}").unwrap();
        let nested = root.join("node_modules").join("dep");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("lib.rs"), "pub fn d() {}\n").unwrap();

        let all: HashSet<String> = walk(root, &[], 524_288)
            .into_iter()
            .map(|f| f.relative)
            .collect();
        assert!(all.contains("keep.rs"));
        assert!(all.contains("notes.md"));
        assert!(all.contains("node_modules/dep/lib.rs")); // descends into deps
        assert!(!all.contains("empty.rs")); // zero-length skipped
        assert!(!all.contains("bin.rs")); // binary skipped
        assert!(!all.contains("ignore.unknown")); // unknown ext skipped
        assert!(!all.iter().any(|r| r.contains(".git"))); // .git pruned

        let rust_only: HashSet<String> = walk(root, &["rust".into()], 524_288)
            .into_iter()
            .map(|f| f.relative)
            .collect();
        assert!(rust_only.contains("keep.rs"));
        assert!(!rust_only.contains("notes.md")); // language filter applied
    }

    #[test]
    fn walk_skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("big.rs"), "x".repeat(1000)).unwrap();
        let found = walk(dir.path(), &[], 500);
        assert!(found.is_empty());
    }
}
