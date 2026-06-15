mod chunk;
mod embed;
mod manifest;
mod store;
mod tschunk;
mod walk;

use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::Result;
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
    /// Pack a built index directory into one gzip-tar archive for download.
    Pack {
        #[arg(long)]
        index: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Index {
            dir,
            ollama,
            model,
            out,
            lang,
            max_bytes,
        } => index(dir, ollama, model, out, lang, max_bytes).await,
        Cmd::Search {
            index,
            query,
            ollama,
            model,
            k,
        } => search(index, query, ollama, model, k).await,
        Cmd::Pack { index, out } => pack(index, out),
    }
}

/// Bundle a built `.rebase-index` directory into a single gzip-tar archive so the
/// app can download it in one `file_get` instead of many. Entries are stored
/// relative to the index dir, so extraction unpacks straight into the target.
fn pack(index: PathBuf, out: PathBuf) -> Result<()> {
    anyhow::ensure!(index.exists(), "index dir not found: {}", index.display());
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = std::fs::File::create(&out)?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);
    tar.append_dir_all(".", &index)?;
    tar.into_inner()?.finish()?;
    eprintln!("packed {} -> {}", index.display(), out.display());
    Ok(())
}

struct Cur {
    entry: walk::FileEntry,
    content: String,
    hash: String,
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

    // Read + hash every current file.
    let mut current: Vec<Cur> = Vec::new();
    for f in walk::walk(&dir, &lang, max_bytes) {
        if let Ok(content) = std::fs::read_to_string(&f.path) {
            let hash = manifest::file_hash(&content);
            current.push(Cur {
                entry: f,
                content,
                hash,
            });
        }
    }

    let manifest_path = out.join("manifest.json");
    let prev = manifest::load(&manifest_path);
    let current_rel: HashSet<&str> = current.iter().map(|c| c.entry.relative.as_str()).collect();

    // Diff against the manifest.
    let changed: Vec<&Cur> = current
        .iter()
        .filter(|c| {
            prev.get(&c.entry.relative)
                .map(|h| h != &c.hash)
                .unwrap_or(true)
        })
        .collect();
    let removed: Vec<String> = prev
        .keys()
        .filter(|k| !current_rel.contains(k.as_str()))
        .cloned()
        .collect();

    let db = store::open_db(&out).await?;
    let has_table = store::table_exists(&db).await?;

    if changed.is_empty() && removed.is_empty() && has_table {
        eprintln!("index up to date — {} files", current.len());
        return Ok(());
    }
    eprintln!(
        "{} changed, {} removed, {} files total",
        changed.len(),
        removed.len(),
        current.len()
    );

    // Chunk + embed only the changed files.
    let mut chunks = Vec::new();
    for c in &changed {
        chunks.extend(chunk::chunk_file(
            &c.entry.relative,
            &c.entry.language,
            &c.content,
        ));
    }
    let vectors = if chunks.is_empty() {
        Vec::new()
    } else {
        eprintln!(
            "embedding {} chunks via {} ({})",
            chunks.len(),
            ollama,
            model
        );
        let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
        embed::Embedder::new(&ollama, &model).embed(&texts).await?
    };
    anyhow::ensure!(vectors.len() == chunks.len(), "embedding count mismatch");

    // Pair chunks with their embeddings, dropping any that couldn't be embedded
    // (the embedder retries individually and yields None for the few that fail,
    // so a handful of bad chunks never aborts the whole index).
    let chunk_count = chunks.len();
    let records: Vec<(chunk::Chunk, Vec<f32>)> = chunks
        .into_iter()
        .zip(vectors)
        .filter_map(|(c, v)| v.map(|vec| (c, vec)))
        .collect();
    if records.len() < chunk_count {
        eprintln!(
            "embedded {} chunks; skipped {} that failed to embed",
            records.len(),
            chunk_count - records.len()
        );
    }
    let dim = records.first().map(|(_, v)| v.len() as i32);

    if has_table || dim.is_some() {
        let table = store::ensure_table(&db, dim.unwrap_or(1)).await?;
        // Replace changed files + drop removed ones, then add fresh chunks.
        let mut to_delete: Vec<String> = changed.iter().map(|c| c.entry.relative.clone()).collect();
        to_delete.extend(removed);
        store::delete_files(&table, &to_delete).await?;
        store::add_chunks(&table, dim.unwrap_or(1), &records).await?;
    }

    // Persist the new manifest.
    let new_manifest: manifest::Manifest = current
        .iter()
        .map(|c| (c.entry.relative.clone(), c.hash.clone()))
        .collect();
    manifest::save(&manifest_path, &new_manifest)?;
    eprintln!("index updated at {}", out.display());
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
