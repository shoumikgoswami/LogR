use tauri::State;
use crate::state::{DriftlogConfig, FlushHandle, PipelineStats, SharedStats};
use crate::synthesis::openrouter::list_openrouter_models as or_list_models;

pub fn load_config_sync() -> DriftlogConfig {
    let path = config_path();
    if !path.exists() {
        return DriftlogConfig::default();
    }
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn config_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".logr")
        .join("config.json")
}

#[tauri::command]
pub fn save_config(config: DriftlogConfig) -> Result<(), String> {
    let path = config_path();
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    tracing::info!("Config saved to {:?}", path);
    Ok(())
}

#[tauri::command]
pub fn load_config() -> Result<DriftlogConfig, String> {
    let path = config_path();
    if !path.exists() {
        return Ok(DriftlogConfig::default());
    }
    let json = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn reset_config() -> Result<(), String> {
    let path = config_path();
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    tracing::info!("Config reset to defaults");
    Ok(())
}

#[tauri::command]
pub fn clear_notes(notes_dir: String) -> Result<u32, String> {
    let dir = std::path::Path::new(&notes_dir);
    if !dir.exists() {
        return Ok(0);
    }
    let mut count = 0u32;
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path).map_err(|e| e.to_string())?;
            count += 1;
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            count += 1;
        }
    }
    tracing::info!("Cleared {} items from notes dir", count);
    Ok(count)
}

#[tauri::command]
pub fn write_test_note() -> Result<String, String> {
    use crate::session::types::{EventType, RawEvent, Session, SessionStatus};
    use crate::writer::MarkdownWriter;

    let config = load_config_sync();
    let notes_dir = std::path::PathBuf::from(&config.notes_dir);
    std::fs::create_dir_all(&notes_dir).map_err(|e| e.to_string())?;
    let writer = MarkdownWriter::new(notes_dir);

    let now = chrono::Utc::now();
    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        started_at: now - chrono::Duration::minutes(5),
        ended_at: Some(now),
        dominant_app: Some("LogR Test".into()),
        topics: vec!["test note".into(), "pipeline check".into()],
        status: SessionStatus::Complete,
        events: vec![
            RawEvent {
                timestamp: now - chrono::Duration::minutes(4),
                event_type: EventType::WindowFocus,
                app: Some("LogR Test".into()),
                title: Some("Verifying note pipeline".into()),
                content: None,
                context: None,
                metadata: Some("dwell=240s context_type=other".into()),
            },
            RawEvent {
                timestamp: now - chrono::Duration::minutes(2),
                event_type: EventType::ClipboardChange,
                app: None,
                title: None,
                content: Some("LogR is working correctly".into()),
                context: None,
                metadata: Some("type=text".into()),
            },
        ],
    };

    let note = writer.write_raw(&session)?;
    tracing::info!("Test note written: {}", note.file_path);
    Ok(note.file_path)
}

#[tauri::command]
pub fn get_status(stats: State<'_, SharedStats>) -> PipelineStats {
    stats.0.lock().unwrap().clone()
}

#[tauri::command]
pub fn flush_session(flush_handle: State<'_, FlushHandle>) -> Result<(), String> {
    let guard = flush_handle.0.lock().map_err(|e| e.to_string())?;
    match guard.as_ref() {
        None => Err("Pipeline not ready yet".into()),
        Some(tx) => tx.try_send(()).map_err(|e| format!("Flush send failed: {e}")),
    }
}

#[tauri::command]
pub async fn check_ollama(url: String) -> Result<bool, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(format!("{}/api/tags", url.trim_end_matches('/')))
        .send()
        .await;
    Ok(resp.map(|r| r.status().is_success()).unwrap_or(false))
}

/// Capture the screen right now and describe it using the configured vision provider.
/// Reads config directly so it always uses the currently saved settings.
#[tauri::command]
pub async fn test_vision() -> Result<String, String> {
    use base64::Engine;
    use crate::collectors::screenshot::{
        capture_primary_screen, ask_vision_ollama_with_error, ask_vision_openrouter_with_error,
    };

    let config = load_config_sync();

    if config.vision_model.trim().is_empty() {
        return Err("No vision model configured — set one in Settings and save first.".into());
    }

    // Capture + encode
    let jpeg_bytes = tokio::task::spawn_blocking(capture_primary_screen)
        .await
        .map_err(|e| format!("spawn_blocking panic: {e}"))?
        .ok_or_else(|| "Screen capture failed — no primary monitor found".to_string())?;

    let kb = jpeg_bytes.len() / 1024;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&jpeg_bytes);

    let result = if config.provider == "openrouter" {
        ask_vision_openrouter_with_error(&config.openrouter_api_key, &config.vision_model, &b64).await
    } else {
        ask_vision_ollama_with_error(&config.ollama_url, &config.vision_model, &b64).await
    };

    match result {
        Ok(desc) => Ok(format!("screenshot={}KB — {}", kb, desc)),
        Err(e)   => Err(format!("screenshot={}KB captured, but: {}", kb, e)),
    }
}

#[tauri::command]
pub async fn list_openrouter_models(api_key: String) -> Result<Vec<String>, String> {
    or_list_models(&api_key).await
}

#[tauri::command]
pub async fn list_ollama_models(url: String) -> Result<Vec<String>, String> {
    #[derive(serde::Deserialize)]
    struct TagsResponse { models: Vec<ModelEntry> }
    #[derive(serde::Deserialize)]
    struct ModelEntry { name: String }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(format!("{}/api/tags", url.trim_end_matches('/')))
        .send()
        .await
        .map_err(|e| format!("Ollama unreachable: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Ollama returned {}", resp.status()));
    }

    let tags: TagsResponse = resp.json().await.map_err(|e| e.to_string())?;
    let mut names: Vec<String> = tags.models.into_iter().map(|m| m.name).collect();
    names.sort();
    Ok(names)
}
