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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_content_yields_no_chunks() {
        assert!(chunk_lines("a.yaml", "yaml", "").is_empty());
    }

    #[test]
    fn short_file_is_one_chunk_with_path_prefix() {
        let chunks = chunk_lines("conf/app.yaml", "yaml", "a: 1\nb: 2\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks[0].line_end, 2);
        assert!(chunks[0].text.starts_with("// conf/app.yaml (yaml)\n"));
        assert!(chunks[0].text.contains("a: 1"));
    }

    #[test]
    fn long_file_windows_with_overlap() {
        let body: String = (1..=140).map(|n| format!("line{n}\n")).collect();
        let chunks = chunk_lines("big.yaml", "yaml", &body);
        // step = WINDOW - OVERLAP = 50; windows start at lines 1, 51, 101.
        assert_eq!(chunks.len(), 3);
        assert_eq!((chunks[0].line_start, chunks[0].line_end), (1, 60));
        assert_eq!((chunks[1].line_start, chunks[1].line_end), (51, 110));
        assert_eq!((chunks[2].line_start, chunks[2].line_end), (101, 140));
        // consecutive windows overlap by OVERLAP lines
        assert_eq!(
            chunks[0].line_end - chunks[1].line_start + 1,
            OVERLAP as u32
        );
    }

    #[test]
    fn chunk_file_falls_back_to_lines_for_unwired_language() {
        // yaml has no tree-sitter grammar wired -> line windows.
        let chunks = chunk_file("a.yaml", "yaml", "x: 1\n");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].language, "yaml");
    }

    #[test]
    fn chunk_hash_is_stable_for_same_text() {
        let a = chunk_lines("a.yaml", "yaml", "k: v\n");
        let b = chunk_lines("a.yaml", "yaml", "k: v\n");
        assert_eq!(a[0].hash, b[0].hash);
        assert_eq!(a[0].hash.len(), 16); // sha256[..8] -> 16 hex chars
    }
}
