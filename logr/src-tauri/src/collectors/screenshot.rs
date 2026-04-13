/// Screenshot capture + vision description (Ollama or OpenRouter).
///
/// Flow:
///   1. Capture the primary monitor at reduced resolution (never written to disk)
///   2. Encode as JPEG in memory → base64
///   3. Send to the configured provider (Ollama /api/generate or OpenRouter /chat/completions)
///   4. Return the text description; drop the image bytes
///
/// All failures are silent — returns None so the rest of the pipeline is unaffected.

use base64::Engine;

const VISION_PROMPT: &str =
    "You are a personal activity logger. Look at this screenshot and describe in 2-3 sentences \
     what the user is currently doing. Be specific: mention the app, document name, website, \
     code being written, article being read, or task visible. Focus on content — what are they \
     reading, writing, or working on? Be concise and factual. No preamble.";

// ── Public provider-aware entry point ────────────────────────────────────────

/// Capture the screen and describe it using whichever provider is configured.
pub async fn describe_screen_for_config(config: crate::state::DriftlogConfig) -> Option<String> {
    if config.vision_model.trim().is_empty() {
        tracing::debug!("[vision] disabled (no model)");
        return None;
    }

    let model = config.vision_model.clone();
    tracing::debug!("[vision] capturing screen, provider={} model={}", config.provider, model);

    let jpeg_bytes = match tokio::task::spawn_blocking(capture_primary_screen).await {
        Ok(Some(b)) => { tracing::debug!("[vision] captured {} bytes", b.len()); b }
        Ok(None)    => { tracing::warn!("[vision] capture returned None"); return None; }
        Err(e)      => { tracing::warn!("[vision] spawn_blocking panicked: {}", e); return None; }
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);

    let result = if config.provider == "openrouter" {
        ask_vision_openrouter_with_error(&config.openrouter_api_key, &model, &b64).await
    } else {
        ask_vision_ollama_with_error(&config.ollama_url, &model, &b64).await
    };

    match result {
        Ok(desc) => {
            tracing::info!("[vision] {}: {}…", config.provider, desc.chars().take(80).collect::<String>());
            Some(desc)
        }
        Err(e) => {
            tracing::warn!("[vision] {}", e);
            None
        }
    }
}

/// Ollama-only entry point kept for backward compat (used by old test_vision path).
pub async fn describe_screen(ollama_url: &str, vision_model: &str) -> Option<String> {
    if vision_model.trim().is_empty() {
        return None;
    }
    let jpeg_bytes = match tokio::task::spawn_blocking(capture_primary_screen).await {
        Ok(Some(b)) => b,
        _ => return None,
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    match ask_vision_ollama_with_error(ollama_url, vision_model, &b64).await {
        Ok(d) => Some(d),
        Err(e) => { tracing::warn!("[vision] {}", e); None }
    }
}

// ── Screen capture ────────────────────────────────────────────────────────────

pub fn capture_primary_screen() -> Option<Vec<u8>> {
    use image::{DynamicImage, ImageFormat};
    use xcap::Monitor;

    let monitors = match Monitor::all() {
        Ok(m) if !m.is_empty() => m,
        Ok(_)  => { tracing::warn!("[vision] no monitors found"); return None; }
        Err(e) => { tracing::warn!("[vision] Monitor::all() failed: {}", e); return None; }
    };
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| Monitor::all().ok()?.into_iter().next())?;

    let rgba = match monitor.capture_image() {
        Ok(img) => img,
        Err(e)  => { tracing::warn!("[vision] capture_image() failed: {}", e); return None; }
    };

    let dynamic = DynamicImage::ImageRgba8(rgba);
    let (w, h) = (dynamic.width(), dynamic.height());
    let tw = 1280u32;
    let th = (h as f32 * (tw as f32 / w as f32)) as u32;
    let resized = dynamic.resize(tw, th, image::imageops::FilterType::Triangle);

    let mut buf = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut buf, ImageFormat::Jpeg).ok()?;
    Some(buf.into_inner())
}

// ── Ollama vision (/api/generate) ─────────────────────────────────────────────

#[derive(serde::Serialize)]
struct OllamaVisionRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    images: Vec<&'a str>,
    stream: bool,
    options: OllamaVisionOptions,
}

#[derive(serde::Serialize)]
struct OllamaVisionOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(serde::Deserialize)]
struct OllamaVisionResponse {
    response: String,
}

pub async fn ask_vision_ollama_with_error(ollama_url: &str, model: &str, b64_image: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("client build: {e}"))?;

    let req = OllamaVisionRequest {
        model,
        prompt: VISION_PROMPT,
        images: vec![b64_image],
        stream: false,
        options: OllamaVisionOptions { temperature: 0.2, num_predict: 150 },
    };

    let resp = client
        .post(format!("{}/api/generate", ollama_url.trim_end_matches('/')))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("Ollama HTTP error: {e}"))?;

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("Ollama {status}: {body}"));
    }

    tracing::debug!("[vision/ollama] raw: {}", &body.chars().take(200).collect::<String>());

    let parsed: OllamaVisionResponse = serde_json::from_str(&body)
        .map_err(|e| format!("Ollama JSON parse ({e}): {}", &body.chars().take(200).collect::<String>()))?;

    let desc = parsed.response.trim().to_string();
    if desc.is_empty() {
        Err("Ollama returned empty response — model may not support vision".into())
    } else {
        Ok(desc)
    }
}

/// Backward-compat alias used by `commands::test_vision` (Ollama path).
pub async fn ask_vision_with_error(ollama_url: &str, model: &str, b64_image: &str) -> Result<String, String> {
    ask_vision_ollama_with_error(ollama_url, model, b64_image).await
}

// ── OpenRouter vision (/chat/completions with image_url) ─────────────────────

pub async fn ask_vision_openrouter_with_error(api_key: &str, model: &str, b64_image: &str) -> Result<String, String> {
    if api_key.trim().is_empty() {
        return Err("OpenRouter API key is not set".into());
    }

    // OpenAI-compatible multimodal message format
    #[derive(serde::Serialize)]
    struct Req<'a> {
        model: &'a str,
        messages: Vec<Msg>,
        max_tokens: u32,
    }
    #[derive(serde::Serialize)]
    struct Msg {
        role: String,
        content: Vec<ContentPart>,
    }
    #[derive(serde::Serialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum ContentPart {
        Text { text: String },
        ImageUrl { image_url: ImageUrl },
    }
    #[derive(serde::Serialize)]
    struct ImageUrl { url: String }

    #[derive(serde::Deserialize)]
    struct Resp { choices: Vec<Choice> }
    #[derive(serde::Deserialize)]
    struct Choice { message: MsgResp }
    #[derive(serde::Deserialize)]
    struct MsgResp { content: String }

    let data_uri = format!("data:image/jpeg;base64,{}", b64_image);

    let body = Req {
        model,
        messages: vec![Msg {
            role: "user".into(),
            content: vec![
                ContentPart::ImageUrl { image_url: ImageUrl { url: data_uri } },
                ContentPart::Text { text: VISION_PROMPT.into() },
            ],
        }],
        max_tokens: 200,
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("client build: {e}"))?;

    let resp = client
        .post("https://openrouter.ai/api/v1/chat/completions")
        .bearer_auth(api_key)
        .header("HTTP-Referer", "https://github.com/shoumikgoswami/LogR")
        .header("X-Title", "LogR")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("OpenRouter HTTP error: {e}"))?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("OpenRouter {status}: {text}"));
    }

    tracing::debug!("[vision/openrouter] raw: {}", &text.chars().take(200).collect::<String>());

    let parsed: Resp = serde_json::from_str(&text)
        .map_err(|e| format!("OpenRouter JSON parse ({e}): {}", &text.chars().take(200).collect::<String>()))?;

    let desc = parsed.choices.into_iter().next()
        .map(|c| c.message.content.trim().to_string())
        .unwrap_or_default();

    if desc.is_empty() {
        Err("OpenRouter returned empty response — model may not support vision".into())
    } else {
        Ok(desc)
    }
}
