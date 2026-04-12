/// Screenshot capture + Ollama vision description.
///
/// Flow:
///   1. Capture the primary monitor at reduced resolution (never written to disk)
///   2. Encode as JPEG in memory → base64
///   3. POST to Ollama /api/chat with the image asking for a brief activity description
///   4. Return the text description; drop the image bytes
///
/// All failures are silent — returns None so the rest of the pipeline is unaffected.

use base64::Engine;

const VISION_PROMPT: &str =
    "You are a personal activity logger. Look at this screenshot and describe in 2-3 sentences \
     what the user is currently doing. Be specific: mention the app, document name, website, \
     code being written, article being read, or task visible. Focus on content — what are they \
     reading, writing, or working on? Be concise and factual. No preamble.";

/// Capture the primary screen and ask Ollama to describe what the user is doing.
/// Returns a plain-text description or None if anything fails.
pub async fn describe_screen(ollama_url: &str, vision_model: &str) -> Option<String> {
    if vision_model.trim().is_empty() {
        tracing::debug!("[vision] disabled (no model)");
        return None;
    }

    tracing::debug!("[vision] capturing screen for model={}", vision_model);

    // Capture in a blocking task (xcap uses OS APIs)
    let jpeg_bytes = match tokio::task::spawn_blocking(capture_primary_screen).await {
        Ok(Some(bytes)) => {
            tracing::debug!("[vision] screen captured: {} bytes JPEG", bytes.len());
            bytes
        }
        Ok(None) => {
            tracing::warn!("[vision] screen capture returned None (xcap/image error)");
            return None;
        }
        Err(e) => {
            tracing::warn!("[vision] spawn_blocking panicked: {}", e);
            return None;
        }
    };

    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);
    tracing::debug!("[vision] sending {} chars of base64 to Ollama", b64.len());

    let result = ask_vision(ollama_url, vision_model, &b64).await;
    match &result {
        Some(desc) => tracing::info!("[vision] got description: {}…", desc.chars().take(80).collect::<String>()),
        None => tracing::warn!("[vision] ask_vision returned None"),
    }
    result
}

// ── Screen capture ────────────────────────────────────────────────────────────

fn capture_primary_screen() -> Option<Vec<u8>> {
    use image::{DynamicImage, ImageFormat};
    use xcap::Monitor;

    let monitors = match Monitor::all() {
        Ok(m) if !m.is_empty() => m,
        Ok(_) => { tracing::warn!("[vision] no monitors found"); return None; }
        Err(e) => { tracing::warn!("[vision] Monitor::all() failed: {}", e); return None; }
    };
    let monitor = monitors
        .into_iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| Monitor::all().ok()?.into_iter().next())?;

    let rgba = match monitor.capture_image() {
        Ok(img) => img,
        Err(e) => { tracing::warn!("[vision] capture_image() failed: {}", e); return None; }
    };

    // Downscale to 1280-wide to keep payload small (~80–150 KB JPEG)
    let dynamic = DynamicImage::ImageRgba8(rgba);
    let (w, h) = (dynamic.width(), dynamic.height());
    let target_w = 1280u32;
    let target_h = (h as f32 * (target_w as f32 / w as f32)) as u32;
    let resized = dynamic.resize(target_w, target_h, image::imageops::FilterType::Triangle);

    let mut buf = std::io::Cursor::new(Vec::new());
    resized
        .write_to(&mut buf, ImageFormat::Jpeg)
        .ok()?;

    Some(buf.into_inner())
}

// ── Ollama vision call (uses /api/generate — wider model support) ─────────────

#[derive(serde::Serialize)]
struct VisionRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    images: Vec<&'a str>,
    stream: bool,
    options: VisionOptions,
}

#[derive(serde::Serialize)]
struct VisionOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(serde::Deserialize)]
struct VisionResponse {
    response: String,
}

/// Returns the description or an error string (for surfacing in the UI).
pub async fn ask_vision_with_error(ollama_url: &str, model: &str, b64_image: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("client build: {e}"))?;

    let req = VisionRequest {
        model,
        prompt: VISION_PROMPT,
        images: vec![b64_image],
        stream: false,
        options: VisionOptions {
            temperature: 0.2,
            num_predict: 150,
        },
    };

    let resp = client
        .post(format!("{}/api/generate", ollama_url.trim_end_matches('/')))
        .json(&req)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {e}"))?;

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();

    if !status.is_success() {
        return Err(format!("Ollama {status}: {body_text}"));
    }

    tracing::debug!("[vision] raw response: {}", &body_text.chars().take(200).collect::<String>());

    let parsed: VisionResponse = serde_json::from_str(&body_text)
        .map_err(|e| format!("JSON parse failed ({e}): {}", &body_text.chars().take(200).collect::<String>()))?;

    let desc = parsed.response.trim().to_string();
    if desc.is_empty() {
        Err("Ollama returned empty response — model may not support vision".into())
    } else {
        Ok(desc)
    }
}

async fn ask_vision(ollama_url: &str, model: &str, b64_image: &str) -> Option<String> {
    match ask_vision_with_error(ollama_url, model, b64_image).await {
        Ok(desc) => Some(desc),
        Err(e) => {
            tracing::warn!("[vision] {}", e);
            None
        }
    }
}
