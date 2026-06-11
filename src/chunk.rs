use sha2::{Digest, Sha256};

pub struct Chunk {
    pub relative: String,
    pub language: String,
    pub line_start: u32,
    pub line_end: u32,
    pub text: String,
    pub hash: String,
}

const WINDOW: usize = 60;
const OVERLAP: usize = 10;

/// Chunk a file: boundary-aware via tree-sitter when a grammar is wired for the
/// language, else line windows. Each chunk is prefixed with its file path so
/// the embedding has a little locating context.
pub fn chunk_file(relative: &str, language: &str, content: &str) -> Vec<Chunk> {
    if let Some(chunks) = crate::tschunk::chunk_with_grammar(relative, language, content) {
        return chunks;
    }
    chunk_lines(relative, language, content)
}

fn chunk_lines(relative: &str, language: &str, content: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let step = WINDOW.saturating_sub(OVERLAP).max(1);
    let mut start = 0;
    while start < lines.len() {
        let end = (start + WINDOW).min(lines.len());
        let body = lines[start..end].join("\n");
        let text = format!("// {relative} ({language})\n{body}");
        let mut hasher = Sha256::new();
        hasher.update(text.as_bytes());
        let hash = hex::encode(&hasher.finalize()[..8]);
        chunks.push(Chunk {
            relative: relative.to_string(),
            language: language.to_string(),
            line_start: (start as u32) + 1,
            line_end: end as u32,
            text,
            hash,
        });
        if end == lines.len() {
            break;
        }
        start += step;
    }
    chunks
}
