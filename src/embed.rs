use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const BATCH: usize = 32;
/// Hard cap on per-input length. node_modules/vendor are indexed on purpose, and
/// they contain huge minified single-line files; Ollama's embed endpoint rejects
/// (or chokes on) over-long inputs, so clamp before sending. `truncate: true`
/// also lets the server truncate to the model's context as a backstop.
const MAX_INPUT_CHARS: usize = 8000;

pub struct Embedder {
    client: reqwest::Client,
    url: String,
    model: String,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
    truncate: bool,
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

fn clamp(text: &str) -> String {
    if text.len() <= MAX_INPUT_CHARS {
        return text.to_string();
    }
    // Clamp on a char boundary at/under the byte cap.
    let mut end = MAX_INPUT_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

impl Embedder {
    pub fn new(ollama_url: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: format!("{}/api/embed", ollama_url.trim_end_matches('/')),
            model: model.to_string(),
        }
    }

    /// Embed many texts, batched. Returns a vector aligned 1:1 with `texts`;
    /// entries that couldn't be embedded (after an individual retry) are `None`
    /// and should be dropped by the caller — one bad chunk must not abort the
    /// whole index (mirrors the original Crucible's resilient embedder).
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Option<Vec<f32>>>> {
        let mut out: Vec<Option<Vec<f32>>> = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            match self.embed_batch(batch).await {
                Ok(vecs) => out.extend(vecs.into_iter().map(Some)),
                Err(err) => {
                    eprintln!("batch embed failed ({err}); retrying individually");
                    for text in batch {
                        match self.embed_batch(std::slice::from_ref(text)).await {
                            Ok(mut v) => out.push(v.pop()),
                            Err(e2) => {
                                eprintln!("  skipping chunk: {e2}");
                                out.push(None);
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// One embed request. Surfaces Ollama's response body on a non-2xx so the
    /// real reason (bad input, missing model, …) is visible instead of a bare
    /// status code.
    async fn embed_batch(&self, batch: &[String]) -> Result<Vec<Vec<f32>>> {
        let inputs: Vec<String> = batch.iter().map(|s| clamp(s)).collect();
        let resp = self
            .client
            .post(&self.url)
            .json(&EmbedRequest {
                model: &self.model,
                input: &inputs,
                truncate: true,
            })
            .send()
            .await
            .with_context(|| format!("POST {} (is Ollama running?)", self.url))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Ollama embed {} → {}: {}", self.url, status, body.trim());
        }
        let parsed = resp
            .json::<EmbedResponse>()
            .await
            .context("parsing Ollama embed response")?;
        Ok(parsed.embeddings)
    }

    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed_batch(&[text.to_string()]).await?;
        v.pop().context("no embedding returned")
    }
}
