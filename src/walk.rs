use std::path::{Path, PathBuf};
use ignore::WalkBuilder;

pub struct FileEntry {
    pub path: PathBuf,
    pub relative: String,
    pub language: String,
}

/// Extra ignore filenames honored alongside `.gitignore`/`.ignore`, so a repo can
/// scope what gets indexed (handy on hosts with a small context window). Listed
/// in increasing precedence; each uses gitignore syntax.
const CUSTOM_IGNORE_FILES: &[&str] = &[
    ".rebaseignore",
    ".aiexclude",   // Gemini Code Assist
    ".aiignore",    // JetBrains AI / generic
    ".codeiumignore",
    ".cursorignore",
];

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

/// Walk `root` recursively, honoring ignore files so indexing focuses on the
/// application's own code: `.gitignore`/`.ignore` (which usually exclude
/// node_modules, vendor, target, dist, …) plus the AI-ignore files in
/// `CUSTOM_IGNORE_FILES`. `.git`, binaries, empty/oversized files, and unknown
/// extensions are always skipped. `langs` (if non-empty) filters to those tags.
pub fn walk(root: &Path, langs: &[String], max_bytes: u64) -> Vec<FileEntry> {
    let mut builder = WalkBuilder::new(root);
    builder
        .follow_links(false)
        .hidden(false) // don't blanket-skip dotfiles; ignore files decide
        .parents(true) // respect ignore files in ancestor dirs
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .ignore(true)
        .require_git(false); // apply .gitignore even outside a git checkout
    for name in CUSTOM_IGNORE_FILES {
        builder.add_custom_ignore_filename(name);
    }

    let mut out = Vec::new();
    for entry in builder.build().filter_map(Result::ok) {
        if entry.file_type().is_none_or(|t| !t.is_file()) {
            continue;
        }
        let path = entry.path();
        // Never index VCS internals (kept hidden(false), so prune explicitly).
        if path.components().any(|c| c.as_os_str() == ".git") {
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
        let path = path.to_path_buf();
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
        // No ignore file present → nothing extra is excluded (deps still walked).
        assert!(all.contains("node_modules/dep/lib.rs"));
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
    fn walk_respects_gitignore_and_custom_ignore_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join(".git")).unwrap(); // mark as a repo
        fs::write(root.join(".gitignore"), "node_modules/\ndist/\n").unwrap();
        fs::write(root.join(".cursorignore"), "secret.rs\n").unwrap();

        fs::write(root.join("app.rs"), "fn main() {}\n").unwrap();
        fs::write(root.join("secret.rs"), "fn s() {}\n").unwrap();
        let nm = root.join("node_modules").join("dep");
        fs::create_dir_all(&nm).unwrap();
        fs::write(nm.join("lib.rs"), "pub fn d() {}\n").unwrap();
        fs::create_dir_all(root.join("dist")).unwrap();
        fs::write(root.join("dist").join("bundle.rs"), "fn b() {}\n").unwrap();

        let found: HashSet<String> = walk(root, &[], 524_288)
            .into_iter()
            .map(|f| f.relative)
            .collect();
        assert!(found.contains("app.rs")); // app code kept
        assert!(!found.contains("node_modules/dep/lib.rs")); // .gitignore
        assert!(!found.contains("dist/bundle.rs")); // .gitignore
        assert!(!found.contains("secret.rs")); // .cursorignore
    }

    #[test]
    fn walk_skips_oversized_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("big.rs"), "x".repeat(1000)).unwrap();
        let found = walk(dir.path(), &[], 500);
        assert!(found.is_empty());
    }
}
