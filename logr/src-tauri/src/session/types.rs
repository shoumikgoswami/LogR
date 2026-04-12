use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    WindowFocus,
    ClipboardChange,
    FileAccess,
    FileEdit,
    /// User was actively typing in an app — keycount only, zero key content captured.
    TypingBurst,
    /// Browser URL changed without the window title changing (SPA navigation, link clicks).
    BrowserNavigation,
    Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub timestamp: DateTime<Utc>,
    pub event_type: EventType,
    pub app: Option<String>,
    pub title: Option<String>,
    /// Primary content: file path for file events, clipboard text for clipboard events.
    pub content: Option<String>,
    /// Supplementary rich context: file snippet, browser URL, running terminal command, etc.
    pub context: Option<String>,
    /// Key=value metadata: dwell=Xs, context_type=editor, type=code, etc.
    pub metadata: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Complete,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub events: Vec<RawEvent>,
    pub dominant_app: Option<String>,
    pub topics: Vec<String>,
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedNote {
    pub session_id: String,
    pub content: String,
    pub file_path: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedEntry {
    pub timestamp: String,
    pub event_type: String,
    pub summary: String,
    pub filtered: bool,
    pub filter_reason: Option<String>,
}
