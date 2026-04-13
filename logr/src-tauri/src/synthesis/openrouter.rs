/// OpenRouter synthesis — uses OpenAI-compatible /chat/completions API.
/// Supports any model available on https://openrouter.ai/models
///
/// Vision: pass base64 image as image_url data URI in message content.

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::session::types::Session;

use super::prompt::build_prompt;

const BASE_URL: &str = "https://openrouter.ai/api/v1";

pub struct OpenRouterClient {
    pub api_key: String,
    pub model: String,
    client: Client,
}

impl OpenRouterClient {
    pub fn new(api_key: String, model: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("reqwest client");
        Self { api_key, model, client }
    }

    /// Synthesize a note from a session. Re-reads model from config each call
    /// so Settings changes take effect without restart.
    pub async fn synthesize_with_model(&self, session: &Session, model: &str) -> Result<String, String> {
        let prompt = build_prompt(session);
        self.chat(model, &prompt).await
    }

    /// Check that the API key is set and the endpoint is reachable.
    pub async fn check_status(&self) -> bool {
        if self.api_key.trim().is_empty() {
            return false;
        }
        let resp = self.client
            .get(format!("{}/models", BASE_URL))
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://github.com/shoumikgoswami/LogR")
            .header("X-Title", "LogR")
            .send()
            .await;
        resp.map(|r| r.status().is_success()).unwrap_or(false)
    }

    async fn chat(&self, model: &str, prompt: &str) -> Result<String, String> {
        #[derive(Serialize)]
        struct Req<'a> {
            model: &'a str,
            messages: Vec<Msg<'a>>,
        }
        #[derive(Serialize)]
        struct Msg<'a> {
            role: &'a str,
            content: &'a str,
        }
        #[derive(Deserialize)]
        struct Resp {
            choices: Vec<Choice>,
        }
        #[derive(Deserialize)]
        struct Choice {
            message: MsgResp,
        }
        #[derive(Deserialize)]
        struct MsgResp {
            content: String,
        }

        let body = Req {
            model,
            messages: vec![Msg { role: "user", content: prompt }],
        };

        let resp = self.client
            .post(format!("{}/chat/completions", BASE_URL))
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://github.com/shoumikgoswami/LogR")
            .header("X-Title", "LogR")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("OpenRouter request failed: {e}"))?;

        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();

        if !status.is_success() {
            return Err(format!("OpenRouter {status}: {text}"));
        }

        let parsed: Resp = serde_json::from_str(&text)
            .map_err(|e| format!("OpenRouter parse error: {e} — {}", &text.chars().take(200).collect::<String>()))?;

        parsed.choices.into_iter().next()
            .map(|c| c.message.content.trim().to_string())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "OpenRouter returned empty response".into())
    }
}

/// Fetch available models from OpenRouter (for Settings dropdown).
pub async fn list_openrouter_models(api_key: &str) -> Result<Vec<String>, String> {
    #[derive(Deserialize)]
    struct Resp { data: Vec<Model> }
    #[derive(Deserialize)]
    struct Model { id: String }

    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(format!("{}/models", BASE_URL))
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/shoumikgoswami/LogR")
        .header("X-Title", "LogR")
        .send()
        .await
        .map_err(|e| format!("OpenRouter unreachable: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("OpenRouter returned {}", resp.status()));
    }

    let data: Resp = resp.json().await.map_err(|e| e.to_string())?;
    let mut ids: Vec<String> = data.data.into_iter().map(|m| m.id).collect();
    ids.sort();
    Ok(ids)
}
