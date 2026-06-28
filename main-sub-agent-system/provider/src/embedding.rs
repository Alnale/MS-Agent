use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use agent_teams_core::memory_store::{EmbeddingError, EmbeddingProvider};

/// OpenAI-compatible embedding provider
pub struct OpenAIEmbeddingProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
    dimensions: usize,
}

impl OpenAIEmbeddingProvider {
    pub fn new(api_key: String, model: Option<String>) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            model: model.unwrap_or_else(|| "text-embedding-3-small".to_string()),
            dimensions: 1536,
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn with_dimensions(mut self, dims: usize) -> Self {
        self.dimensions = dims;
        self
    }
}

#[derive(Serialize)]
struct EmbeddingRequest {
    input: Vec<String>,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
}

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    index: usize,
}

#[async_trait]
impl EmbeddingProvider for OpenAIEmbeddingProvider {
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, EmbeddingError> {
        let results = self.embed_batch(&[text]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::InvalidResponse("Empty response".to_string()))
    }

    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
        let request = EmbeddingRequest {
            input: texts.iter().map(|s| s.to_string()).collect(),
            model: self.model.clone(),
            dimensions: Some(self.dimensions),
        };

        let url = format!("{}/embeddings", self.base_url);
        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbeddingError::Unavailable(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return match status.as_u16() {
                401 => Err(EmbeddingError::Auth(body)),
                429 => Err(EmbeddingError::RateLimited),
                _ => Err(EmbeddingError::Other(format!("{}: {}", status, body))),
            };
        }

        let resp: EmbeddingResponse = response
            .json()
            .await
            .map_err(|e| EmbeddingError::InvalidResponse(e.to_string()))?;

        // Sort by index to maintain order
        let mut embeddings = vec![vec![]; texts.len()];
        for data in resp.data {
            if data.index < embeddings.len() {
                embeddings[data.index] = data.embedding;
            }
        }

        Ok(embeddings)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

/// Ollama embedding provider (local models)
pub struct OllamaEmbeddingProvider {
    client: Client,
    base_url: String,
    model: String,
    dimensions: usize,
}

impl OllamaEmbeddingProvider {
    pub fn new(model: String, base_url: Option<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.unwrap_or_else(|| "http://localhost:11434".to_string()),
            model,
            dimensions: 768, // nomic-embed-text default
        }
    }

    pub fn with_dimensions(mut self, dims: usize) -> Self {
        self.dimensions = dims;
        self
    }
}

#[derive(Serialize)]
struct OllamaEmbedRequest {
    model: String,
    input: String,
}

#[derive(Deserialize)]
struct OllamaEmbedResponse {
    embeddings: Vec<Vec<f32>>,
}

#[async_trait]
impl EmbeddingProvider for OllamaEmbeddingProvider {
    async fn embed(&self, text: &str) -> std::result::Result<Vec<f32>, EmbeddingError> {
        let request = OllamaEmbedRequest {
            model: self.model.clone(),
            input: text.to_string(),
        };

        let url = format!("{}/api/embed", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| EmbeddingError::Unavailable(e.to_string()))?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(EmbeddingError::Other(body));
        }

        let resp: OllamaEmbedResponse = response
            .json()
            .await
            .map_err(|e| EmbeddingError::InvalidResponse(e.to_string()))?;

        resp.embeddings
            .into_iter()
            .next()
            .ok_or_else(|| EmbeddingError::InvalidResponse("Empty embeddings".to_string()))
    }

    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> std::result::Result<Vec<Vec<f32>>, EmbeddingError> {
        // Ollama doesn't support batch embedding natively, do sequentially
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_config() {
        let provider = OpenAIEmbeddingProvider::new("test-key".to_string(), None);
        assert_eq!(provider.model_id(), "text-embedding-3-small");
        assert_eq!(provider.dimensions(), 1536);
    }

    #[test]
    fn test_ollama_provider_config() {
        let provider = OllamaEmbeddingProvider::new("nomic-embed-text".to_string(), None);
        assert_eq!(provider.model_id(), "nomic-embed-text");
        assert_eq!(provider.dimensions(), 768);
    }
}
