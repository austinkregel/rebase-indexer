use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const BATCH: usize = 32;

pub struct Embedder {
    client: reqwest::Client,
    url: String,
    model: String,
}

#[derive(Serialize)]
struct EmbedRequest<'a> {
    model: &'a str,
    input: &'a [String],
}

#[derive(Deserialize)]
struct EmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

impl Embedder {
    pub fn new(ollama_url: &str, model: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            url: format!("{}/api/embed", ollama_url.trim_end_matches('/')),
            model: model.to_string(),
        }
    }

    /// Embed many texts, batched. Documents and queries must go through the same
    /// model so the vectors are comparable.
    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            let resp = self
                .client
                .post(&self.url)
                .json(&EmbedRequest {
                    model: &self.model,
                    input: batch,
                })
                .send()
                .await
                .with_context(|| format!("POST {} (is Ollama running?)", self.url))?
                .error_for_status()
                .context("Ollama embed returned an error status")?
                .json::<EmbedResponse>()
                .await
                .context("parsing Ollama embed response")?;
            out.extend(resp.embeddings);
        }
        Ok(out)
    }

    pub async fn embed_one(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = self.embed(&[text.to_string()]).await?;
        v.pop().context("no embedding returned")
    }
}
