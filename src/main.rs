mod chunk;
mod embed;
mod store;
mod walk;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

/// Builds a per-project LanceDB code index for rebase, embedding via Ollama.
#[derive(Parser)]
#[command(name = "rebase-indexer")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Index a folder (descends into deps) into a LanceDB at <dir>/.rebase-index.
    Index {
        dir: PathBuf,
        #[arg(long, default_value = "http://localhost:11434")]
        ollama: String,
        #[arg(long, default_value = "nomic-embed-text")]
        model: String,
        #[arg(long)]
        out: Option<PathBuf>,
        /// Comma-separated language tags to include (default: all known).
        #[arg(long, value_delimiter = ',')]
        lang: Vec<String>,
        #[arg(long, default_value_t = 524288)]
        max_bytes: u64,
    },
    /// Semantic search against an existing index.
    Search {
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        query: String,
        #[arg(long, default_value = "http://localhost:11434")]
        ollama: String,
        #[arg(long, default_value = "nomic-embed-text")]
        model: String,
        #[arg(long, default_value_t = 10)]
        k: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Index { dir, ollama, model, out, lang, max_bytes } => {
            index(dir, ollama, model, out, lang, max_bytes).await
        }
        Cmd::Search { index, query, ollama, model, k } => {
            search(index, query, ollama, model, k).await
        }
    }
}

async fn index(
    dir: PathBuf,
    ollama: String,
    model: String,
    out: Option<PathBuf>,
    lang: Vec<String>,
    max_bytes: u64,
) -> Result<()> {
    let out = out.unwrap_or_else(|| dir.join(".rebase-index"));
    let files = walk::walk(&dir, &lang, max_bytes);
    eprintln!("scanning {} files under {}", files.len(), dir.display());

    let mut chunks = Vec::new();
    for f in &files {
        let Ok(content) = std::fs::read_to_string(&f.path) else { continue };
        chunks.extend(chunk::chunk_file(&f.relative, &f.language, &content));
    }
    anyhow::ensure!(!chunks.is_empty(), "no indexable files found under {}", dir.display());
    eprintln!("embedding {} chunks via {} ({})", chunks.len(), ollama, model);

    let embedder = embed::Embedder::new(&ollama, &model);
    let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
    let vectors = embedder.embed(&texts).await?;
    anyhow::ensure!(vectors.len() == chunks.len(), "embedding count mismatch");
    let dim = vectors.first().map(|v| v.len() as i32).context("empty embedding")?;

    let records: Vec<(chunk::Chunk, Vec<f32>)> = chunks.into_iter().zip(vectors).collect();
    store::build(&out, dim, &records).await?;
    eprintln!("wrote {}-dim index to {}", dim, out.display());
    Ok(())
}

async fn search(
    index: PathBuf,
    query: String,
    ollama: String,
    model: String,
    k: usize,
) -> Result<()> {
    let embedder = embed::Embedder::new(&ollama, &model);
    let qv = embedder.embed_one(&query).await?;
    for h in store::search(&index, qv, k).await? {
        println!(
            "{}  {}:{}-{}  (d={:.3})",
            h.language, h.relative, h.line_start, h.line_end, h.distance
        );
        let snippet = h.text.lines().take(3).collect::<Vec<_>>().join("\n    ");
        println!("    {snippet}\n");
    }
    Ok(())
}
