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
        let Ok(head) = std::fs::read(&path) else { continue };
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
