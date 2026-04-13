use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::prompt::build_prompt;

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelEntry>,
}
#[derive(Deserialize)]
struct ModelEntry {
    name: String,
}

use crate::session::types::Session;

pub struct OllamaConfig {
    pub model: String,
    pub base_url: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            model: "gemma3:4b".into(),
            base_url: "http://localhost:11434".into(),
            temperature: 0.3,
            max_tokens: 768,
        }
    }
}

#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize, Deserialize)]
struct OllamaMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Deserialize)]
struct OllamaResponse {
    message: OllamaMessage,
}

pub struct OllamaClient {
    pub config: OllamaConfig,
    /// Short-timeout client for status checks.
    check_client: Client,
    /// Long-timeout client for inference (model cold-start can take a while).
    infer_client: Client,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
        let check_client = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("failed to build check HTTP client");
        let infer_client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build inference HTTP client");
        Self { config, check_client, infer_client }
    }

    /// Returns (ollama_running, model_pulled) using the client's own config model.
    pub async fn check_status(&self) -> (bool, bool) {
        self.check_status_for_model(&self.config.model.clone()).await
    }

    /// Returns (ollama_running, model_pulled) for an arbitrary model name.
    /// Use this when the user may have changed the model in Settings after startup.
    pub async fn check_status_for_model(&self, model: &str) -> (bool, bool) {
        let resp = self
            .check_client
            .get(format!("{}/api/tags", self.config.base_url))
            .send()
            .await;

        match resp {
            Ok(r) if r.status().is_success() => {
                let tags: TagsResponse = r.json().await.unwrap_or(TagsResponse { models: vec![] });
                let model_name = model.trim_end_matches(":latest");
                let has_model = tags
                    .models
                    .iter()
                    .any(|m| m.name.starts_with(model_name));
                if !has_model {
                    tracing::warn!(
                        "Ollama running but model '{}' not found. Run: ollama pull {}",
                        model, model
                    );
                }
                (true, has_model)
            }
            _ => (false, false),
        }
    }

    /// Synthesize using the client's own config model.
    pub async fn synthesize(&self, session: &Session) -> Result<String, String> {
        self.synthesize_with_model(session, &self.config.model.clone()).await
    }

    /// Synthesize using an explicit model name (picks up Settings changes without restart).
    pub async fn synthesize_with_model(&self, session: &Session, model: &str) -> Result<String, String> {
        let prompt = build_prompt(session);

        let req = OllamaRequest {
            model: model.to_string(),
            messages: vec![OllamaMessage {
                role: "user".into(),
                content: prompt,
            }],
            stream: false,
            options: OllamaOptions {
                temperature: self.config.temperature,
                num_predict: self.config.max_tokens,
            },
        };

        tracing::debug!("Synthesizing with model '{}'", model);

        let resp = self
            .infer_client
            .post(format!("{}/api/chat", self.config.base_url))
            .json(&req)
            .send()
            .await
            .map_err(|e| format!("Ollama request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Ollama returned {}: {}", status, body));
        }

        let body: OllamaResponse = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse Ollama response: {}", e))?;

        Ok(body.message.content.trim().to_string())
    }
}

