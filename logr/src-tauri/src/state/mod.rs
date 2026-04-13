use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::mpsc;

use crate::session::types::{FeedEntry, GeneratedNote, Session};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DriftlogConfig {
    /// "ollama" or "openrouter"
    #[serde(default = "default_provider")]
    pub provider: String,

    // ── Ollama ────────────────────────────────────────────────────
    pub ollama_model: String,
    pub ollama_url: String,

    // ── OpenRouter ───────────────────────────────────────────────
    #[serde(default)]
    pub openrouter_api_key: String,
    #[serde(default = "default_openrouter_model")]
    pub openrouter_model: String,

    // ── Vision ───────────────────────────────────────────────────
    /// Vision model for screenshot descriptions. Empty = disabled.
    #[serde(default)]
    pub vision_model: String,

    pub notes_dir: String,
    pub session_idle_timeout_secs: u64,
    pub min_dwell_secs: u64,
    pub blocked_apps: Vec<String>,
    pub watch_dirs: Vec<String>,
    pub watch_communication_apps: bool,
}

fn default_provider() -> String { "ollama".into() }
fn default_openrouter_model() -> String { "google/gemini-2.0-flash-001".into() }

impl Default for DriftlogConfig {
    fn default() -> Self {
        let notes_dir = dirs::document_dir()
            .unwrap_or_default()
            .join("LogR")
            .to_string_lossy()
            .into_owned();

        Self {
            provider: "ollama".into(),
            ollama_model: "gemma3:4b".into(),
            ollama_url: "http://localhost:11434".into(),
            openrouter_api_key: String::new(),
            openrouter_model: "google/gemini-2.0-flash-001".into(),
            vision_model: String::new(),
            notes_dir,
            session_idle_timeout_secs: 120,
            min_dwell_secs: 10,
            blocked_apps: vec![],
            watch_dirs: vec![],
            watch_communication_apps: false,
        }
    }
}

pub struct AppState {
    pub is_watching: bool,
    pub session_buffer: Option<Session>,
    pub config: DriftlogConfig,
    pub recent_feed: VecDeque<FeedEntry>,
    pub last_note: Option<GeneratedNote>,
    pub ollama_available: bool,
    pub notes_dir: PathBuf,
}

impl AppState {
    pub fn new() -> Self {
        let config = DriftlogConfig::default();
        let notes_dir = PathBuf::from(&config.notes_dir);
        Self {
            is_watching: true,
            session_buffer: None,
            config,
            recent_feed: VecDeque::with_capacity(50),
            last_note: None,
            ollama_available: false,
            notes_dir,
        }
    }
}

/// Held in Tauri managed state so tray/commands can trigger a session flush.
pub struct FlushHandle(pub Mutex<Option<mpsc::Sender<()>>>);

/// Live pipeline stats readable from the dashboard.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PipelineStats {
    pub events_in_session: usize,
    pub total_notes: u32,
    /// How many window events had a successful vision screenshot description.
    pub vision_snapshots: u32,
    /// Ollama process is reachable at all.
    pub ollama_running: bool,
    /// The configured model has been pulled and is ready.
    pub model_available: bool,
    pub is_watching: bool,
    pub last_note_path: Option<String>,
}

pub struct SharedStats(pub Mutex<PipelineStats>);

impl SharedStats {
    pub fn new() -> Self {
        Self(Mutex::new(PipelineStats {
            events_in_session: 0,
            total_notes: 0,
            vision_snapshots: 0,
            ollama_running: false,
            model_available: false,
            is_watching: true,
            last_note_path: None,
        }))
    }
}
