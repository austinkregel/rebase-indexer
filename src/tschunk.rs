use sha2::{Digest, Sha256};
use tree_sitter::{Language, Parser};

use crate::chunk::Chunk;

/// Tree-sitter grammar for a language tag, or None to fall back to line chunks.
fn grammar_for(language: &str) -> Option<Language> {
    Some(match language {
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        "python" => tree_sitter_python::LANGUAGE.into(),
        "javascript" => tree_sitter_javascript::LANGUAGE.into(),
        "typescript" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "php" => tree_sitter_php::LANGUAGE_PHP.into(),
        "java" => tree_sitter_java::LANGUAGE.into(),
        "c" => tree_sitter_c::LANGUAGE.into(),
        "cpp" => tree_sitter_cpp::LANGUAGE.into(),
        "ruby" => tree_sitter_ruby::LANGUAGE.into(),
        "css" => tree_sitter_css::LANGUAGE.into(),
        "html" => tree_sitter_html::LANGUAGE.into(),
        "elixir" => tree_sitter_elixir::LANGUAGE.into(),
        "erlang" => tree_sitter_erlang::LANGUAGE.into(),
        _ => return None,
    })
}

/// Boundary-aware chunking: one chunk per top-level named construct (function,
/// type, import, …). Returns None when no grammar is wired for the language
/// (caller falls back to line windows).
pub fn chunk_with_grammar(relative: &str, language: &str, content: &str) -> Option<Vec<Chunk>> {
    let lang = grammar_for(language)?;
    let mut parser = Parser::new();
    parser.set_language(&lang).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();
    let bytes = content.as_bytes();

    let mut cursor = root.walk();
    let mut chunks = Vec::new();
    for node in root.children(&mut cursor) {
        if !node.is_named() {
            continue;
        }
        let body = std::str::from_utf8(&bytes[node.byte_range()]).unwrap_or("");
        if body.trim().is_empty() {
            continue;
        }
        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;
        let text = format!("// {relative} ({language})\n{body}");
        let mut h = Sha256::new();
        h.update(text.as_bytes());
        chunks.push(Chunk {
            relative: relative.to_string(),
            language: language.to_string(),
            line_start,
            line_end,
            text,
            hash: hex::encode(&h.finalize()[..8]),
        });
    }
    if chunks.is_empty() {
        None
    } else {
        Some(chunks)
    }
}
