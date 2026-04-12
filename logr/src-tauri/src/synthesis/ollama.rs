use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct TagsResponse {
    models: Vec<ModelEntry>,
}
#[derive(Deserialize)]
struct ModelEntry {
    name: String,
}

use crate::session::types::{EventType, Session};

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

// ── Prompt builder ────────────────────────────────────────────────────────────

fn build_prompt(session: &Session) -> String {
    let start = session.started_at.format("%H:%M").to_string();
    let end = session
        .ended_at
        .unwrap_or_else(Utc::now)
        .format("%H:%M")
        .to_string();
    let duration = session
        .ended_at
        .map(|e| (e - session.started_at).num_minutes())
        .unwrap_or(0);

    let category = crate::session::classify_app(
        session.dominant_app.as_deref().unwrap_or(""),
    );

    // Build a rich event log so the LLM knows exactly what happened
    let event_lines = format_events_for_prompt(session);

    format!(
        r#"You are a personal knowledge logger. Write a concise markdown note summarising what the user just did on their computer.

Rules:
- Be specific: mention the actual apps, window titles, file names, and code/text snippets you can see in the data
- Lines prefixed with → are vision screenshot descriptions — use these to add concrete detail about what was on screen
- Focus on WHAT they were working on and WHY (infer intent from context — e.g. "debugging X", "reading docs for Y", "writing Z")
- Group related activities; skip trivial window switches
- Maximum 8 bullet points
- First line must be a bold heading that captures the main activity (e.g. **Debugging auth flow in VS Code**)
- Do not mention that you are an AI or that you are summarising
- Output only markdown — no preamble, no explanation

--- Session ---
Time: {start} – {end} ({duration} min)
Primary app: {app}
Category: {category:?}
Topics detected: {topics}

--- Activity log ---
{events}
--- End ---

Write the markdown note now."#,
        start = start,
        end = end,
        duration = duration,
        app = session.dominant_app.as_deref().unwrap_or("Unknown"),
        category = category,
        topics = if session.topics.is_empty() { "(none detected)".into() } else { session.topics.join(", ") },
        events = event_lines,
    )
}

/// Format all session events into a human-readable log for the LLM.
fn format_events_for_prompt(session: &Session) -> String {
    let mut lines = Vec::new();

    for event in &session.events {
        let ts = event.timestamp.format("%H:%M:%S").to_string();
        let meta = event.metadata.as_deref().unwrap_or("");
        let line = match &event.event_type {
            EventType::WindowFocus => {
                let app = event.app.as_deref().unwrap_or("?");
                let title = event.title.as_deref().unwrap_or("(no title)");
                let dwell = parse_dwell(meta);
                let context_type = parse_meta_value(meta, "context_type").unwrap_or("other");
                let doc = event.content.as_deref().unwrap_or(title);

                // Base line: what app, what document/page, how long
                let mut line = format!("{} [{}] {} — {} ({}s)", ts, context_type, app, doc, dwell);

                // Append rich context: URL for browsers, running command for terminals
                if let Some(ctx) = &event.context {
                    line.push_str(&format!("\n         → {}", ctx));
                }
                line
            }
            EventType::ClipboardChange => {
                let content = event.content.as_deref().unwrap_or("");
                let content_type = parse_meta_value(meta, "type").unwrap_or("text");
                let preview: String = content.chars().take(200).collect();
                let ellipsis = if content.len() > 200 { "…" } else { "" };
                format!("{} [clipboard/{}] \"{}{}\"", ts, content_type, preview, ellipsis)
            }
            EventType::FileEdit => {
                let path = event.content.as_deref().unwrap_or("?");
                let p = std::path::Path::new(path);
                let filename = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
                let dir = p.parent().and_then(|d| d.file_name()).map(|n| format!("{}/", n.to_string_lossy())).unwrap_or_default();
                let ext = p.extension().map(|e| format!(" ({})", e.to_string_lossy())).unwrap_or_default();
                let mut line = format!("{} [file edited] {}{}{}", ts, dir, filename, ext);
                // Include file snippet so the LLM knows what was actually written
                if let Some(snippet) = &event.context {
                    let preview: String = snippet.chars().take(300).collect();
                    let ellipsis = if snippet.len() > 300 { "…" } else { "" };
                    line.push_str(&format!("\n         Content:\n```\n{}{}\n```", preview, ellipsis));
                }
                line
            }
            EventType::FileAccess => {
                let path = event.content.as_deref().unwrap_or("?");
                let p = std::path::Path::new(path);
                let filename = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_string());
                let dir = p.parent().and_then(|d| d.file_name()).map(|n| format!("{}/", n.to_string_lossy())).unwrap_or_default();
                format!("{} [file opened] {}{}", ts, dir, filename)
            }
            EventType::TypingBurst => {
                let keys = parse_meta_value(meta, "keystrokes").unwrap_or("?");
                let dur = parse_meta_value(meta, "duration").unwrap_or("?");
                let app = event.app.as_deref().unwrap_or("?");
                let title = event.title.as_deref().unwrap_or("?");
                format!("{} [typing] {} keystrokes over {}s in {} ({})", ts, keys, dur, app, title)
            }
            EventType::BrowserNavigation => {
                let url = event.content.as_deref().unwrap_or("?");
                let title = event.title.as_deref().unwrap_or("?");
                format!("{} [navigation] {} — {}", ts, title, url)
            }
            EventType::Idle => continue,
        };
        lines.push(line);
    }

    if lines.is_empty() {
        "(no events)".into()
    } else {
        lines.join("\n")
    }
}

fn parse_dwell(meta: &str) -> u64 {
    // metadata format: "dwell=45s context_type=editor"
    for part in meta.split_whitespace() {
        if let Some(val) = part.strip_prefix("dwell=") {
            let digits: String = val.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>() {
                return n;
            }
        }
    }
    0
}

fn parse_meta_value<'a>(meta: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    for part in meta.split_whitespace() {
        if let Some(val) = part.strip_prefix(prefix.as_str()) {
            return Some(val);
        }
    }
    None
}
