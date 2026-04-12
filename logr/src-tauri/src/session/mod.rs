pub mod types;

pub use types::*;

use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::prefilter::rules::{apply, FilterResult};

// ──────────────────────────────────────────
// App Categories
// ──────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum AppCategory {
    Coding,
    Browser,
    Terminal,
    Writing,
    Communication,
    Design,
    Other,
}

pub fn classify_app(app: &str) -> AppCategory {
    let lower = app.to_lowercase();
    match lower.as_str() {
        // Coding
        a if a.contains("code")
            || a.contains("cursor")
            || a.contains("zed")
            || a.contains("xcode")
            || a.contains("intellij")
            || a.contains("rider")
            || a.contains("goland")
            || a.contains("pycharm")
            || a.contains("webstorm")
            || a.contains("clion")
            || a.contains("rustrover")
            || a.contains("sublime")
            || a.contains("neovim")
            || a.contains("nvim")
            || a.contains("vim")
            || a.contains("emacs")
            || a.contains("helix") =>
        {
            AppCategory::Coding
        }

        // Browser
        a if a.contains("chrome")
            || a.contains("firefox")
            || a.contains("safari")
            || a.contains("arc")
            || a.contains("brave")
            || a.contains("edge")
            || a.contains("opera") =>
        {
            AppCategory::Browser
        }

        // Terminal
        a if a.contains("terminal")
            || a.contains("iterm")
            || a.contains("warp")
            || a.contains("kitty")
            || a.contains("alacritty")
            || a.contains("hyper")
            || a.contains("cmd")
            || a.contains("powershell")
            || a.contains("wezterm") =>
        {
            AppCategory::Terminal
        }

        // Writing
        a if a.contains("notion")
            || a.contains("obsidian")
            || a.contains("word")
            || a.contains("pages")
            || a.contains("bear")
            || a.contains("typora")
            || a.contains("logseq")
            || a.contains("roam")
            || a.contains("craft") =>
        {
            AppCategory::Writing
        }

        // Communication
        a if a.contains("slack")
            || a.contains("discord")
            || a.contains("teams")
            || a.contains("mail")
            || a.contains("outlook")
            || a.contains("zoom")
            || a.contains("telegram")
            || a.contains("signal")
            || a.contains("messages") =>
        {
            AppCategory::Communication
        }

        // Design
        a if a.contains("figma")
            || a.contains("sketch")
            || a.contains("photoshop")
            || a.contains("illustrator")
            || a.contains("affinity")
            || a.contains("canva")
            || a.contains("framer") =>
        {
            AppCategory::Design
        }

        _ => AppCategory::Other,
    }
}

// ──────────────────────────────────────────
// Topic extraction
// ──────────────────────────────────────────

pub fn extract_topics(session: &Session) -> Vec<String> {
    let mut topics: Vec<String> = Vec::new();

    for event in &session.events {
        match &event.event_type {
            EventType::WindowFocus => {
                if let Some(title) = &event.title {
                    // Split on common separators and take each meaningful segment
                    let parts: Vec<&str> = title
                        .split(&['/', '\\', '—', '–', '|'][..])
                        .collect();
                    for part in parts {
                        let trimmed = part.trim();
                        if is_useful_topic(trimmed) {
                            topics.push(trimmed.to_string());
                        }
                    }
                }
            }
            EventType::FileAccess | EventType::FileEdit => {
                if let Some(path_str) = &event.content {
                    let path = Path::new(path_str);
                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy().into_owned();
                        // Skip LogR's own output files and date folders
                        if is_useful_topic(&name) && !looks_like_logr_filename(&name) {
                            topics.push(name);
                        }
                    }
                    // Include the project/parent directory name as context
                    if let Some(parent) = path.parent().and_then(|p| p.file_name()) {
                        let dir = parent.to_string_lossy().into_owned();
                        if is_useful_topic(&dir) && !looks_like_logr_filename(&dir) {
                            topics.push(dir);
                        }
                    }
                }
            }
            EventType::BrowserNavigation => {
                if let Some(url) = &event.content {
                    // Extract domain as a topic
                    if let Some(host) = url.split('/').nth(2) {
                        let domain = host.trim_start_matches("www.");
                        if is_useful_topic(domain) {
                            topics.push(domain.to_string());
                        }
                    }
                }
            }
            // Clipboard and typing bursts are excluded from topics — too noisy.
            EventType::ClipboardChange | EventType::TypingBurst | EventType::Idle => {}
        }
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    topics.retain(|t| seen.insert(t.clone()));
    topics.truncate(10);
    topics
}

/// Returns true if a string is worth using as a topic label.
fn is_useful_topic(s: &str) -> bool {
    let len = s.len();
    if len < 3 || len > 80 {
        return false;
    }
    // Skip things that are entirely digits (dates, numbers)
    if s.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '_') {
        return false;
    }
    // Skip things that look like timestamps HH:MM or HH-MM
    if s.len() == 5 && s.chars().nth(2).map(|c| c == ':' || c == '-').unwrap_or(false) {
        return false;
    }
    true
}

/// Detect filenames that look like LogR's own output (e.g. "14-23_coding_vscode_raw").
fn looks_like_logr_filename(s: &str) -> bool {
    // LogR filenames start with HH-MM_ pattern
    if s.len() > 5 {
        let prefix = &s[..5];
        let looks_like_time = prefix.chars().enumerate().all(|(i, c)| match i {
            0 | 1 => c.is_ascii_digit(),
            2 => c == '-',
            3 | 4 => c.is_ascii_digit(),
            _ => true,
        });
        if looks_like_time && s.contains('_') {
            return true;
        }
    }
    // Date folder pattern YYYY-MM-DD
    if s.len() == 10 {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() == 3 && parts.iter().all(|p| p.chars().all(|c| c.is_ascii_digit())) {
            return true;
        }
    }
    // The index file
    if s == "index" {
        return true;
    }
    false
}

// ──────────────────────────────────────────
// SessionBuffer
// ──────────────────────────────────────────

pub struct SessionBuffer {
    current: Session,
    current_category: AppCategory,
    last_event_time: Instant,
    idle_timeout: Duration,
    session_tx: mpsc::Sender<Session>,
    feed_tx: mpsc::Sender<FeedEntry>,
    event_count: std::sync::Arc<std::sync::Mutex<usize>>,
    /// Cumulative count of window events that had a vision screenshot description.
    vision_count: std::sync::Arc<std::sync::Mutex<u32>>,
}

impl SessionBuffer {
    pub fn new(
        session_tx: mpsc::Sender<Session>,
        feed_tx: mpsc::Sender<FeedEntry>,
        idle_timeout_secs: u64,
        event_count: std::sync::Arc<std::sync::Mutex<usize>>,
        vision_count: std::sync::Arc<std::sync::Mutex<u32>>,
    ) -> Self {
        Self {
            current: new_session(),
            current_category: AppCategory::Other,
            last_event_time: Instant::now(),
            idle_timeout: Duration::from_secs(idle_timeout_secs),
            session_tx,
            feed_tx,
            event_count,
            vision_count,
        }
    }

    /// Process one incoming raw event: run pre-filter, check boundaries, buffer.
    pub async fn process(&mut self, event: RawEvent) {
        let feed_entry_base = FeedEntry {
            timestamp: event.timestamp.format("%H:%M:%S").to_string(),
            event_type: format!("{:?}", event.event_type),
            summary: build_summary(&event),
            filtered: false,
            filter_reason: None,
        };

        // Run pre-filter
        match apply(&event) {
            FilterResult::Drop(reason) => {
                let _ = self
                    .feed_tx
                    .send(FeedEntry {
                        filtered: true,
                        filter_reason: Some(reason.clone()),
                        ..feed_entry_base
                    })
                    .await;
                tracing::debug!("Dropped event: {}", reason);
                return;
            }
            FilterResult::Allow => {}
        }

        // Count vision-enriched window events
        if matches!(event.event_type, EventType::WindowFocus) {
            if event.metadata.as_deref()
                .map(|m| m.contains("vision=true"))
                .unwrap_or(false)
            {
                if let Ok(mut vc) = self.vision_count.lock() {
                    *vc += 1;
                }
            }
        }

        // Check idle timeout
        if self.last_event_time.elapsed() > self.idle_timeout {
            tracing::info!("Session flush: idle timeout");
            self.flush_current(SessionFlushReason::IdleTimeout).await;
        }

        // Check category change
        let incoming_category = event
            .app
            .as_deref()
            .map(classify_app)
            .unwrap_or(AppCategory::Other);

        if incoming_category != self.current_category
            && incoming_category != AppCategory::Other
            && self.current_category != AppCategory::Other
            && !self.current.events.is_empty()
        {
            tracing::info!(
                "Session flush: category change {:?} → {:?}",
                self.current_category,
                incoming_category
            );
            self.flush_current(SessionFlushReason::CategoryChange).await;
        }

        // Update dominant app
        if event.app.is_some() {
            self.current.dominant_app = event.app.clone();
            self.current_category = incoming_category;
        }

        self.last_event_time = Instant::now();
        self.current.events.push(event);

        // Update shared event count for status display
        if let Ok(mut c) = self.event_count.lock() {
            *c = self.current.events.len();
        }

        // Emit feed entry
        let _ = self.feed_tx.send(feed_entry_base).await;

        // Force flush at 30 events
        if self.current.events.len() >= 30 {
            tracing::info!("Session flush: 30-event cap");
            self.flush_current(SessionFlushReason::EventCap).await;
        }
    }

    /// Called by the idle ticker to check if a timeout has silently expired.
    pub async fn check_idle(&mut self) {
        if !self.current.events.is_empty()
            && self.last_event_time.elapsed() > self.idle_timeout
        {
            tracing::info!("Session flush: idle check (no new events)");
            self.flush_current(SessionFlushReason::IdleTimeout).await;
        }
    }

    /// Manually flush the current session (e.g. user-requested).
    pub async fn force_flush(&mut self) {
        if !self.current.events.is_empty() {
            self.flush_current(SessionFlushReason::Manual).await;
        }
    }

    async fn flush_current(&mut self, _reason: SessionFlushReason) {
        if self.current.events.is_empty() {
            return;
        }

        let mut session = std::mem::replace(&mut self.current, new_session());
        session.ended_at = Some(chrono::Utc::now());
        session.topics = extract_topics(&session);
        session.status = SessionStatus::Complete;

        tracing::info!(
            "Flushing session {} | {} events | app={:?} | topics={:?}",
            &session.id[..8],
            session.events.len(),
            session.dominant_app,
            session.topics
        );

        let _ = self.session_tx.send(session).await;
        self.current_category = AppCategory::Other;

        // Reset shared count
        if let Ok(mut c) = self.event_count.lock() {
            *c = 0;
        }
    }
}

enum SessionFlushReason {
    IdleTimeout,
    CategoryChange,
    EventCap,
    Manual,
}

fn new_session() -> Session {
    Session {
        id: Uuid::new_v4().to_string(),
        started_at: chrono::Utc::now(),
        ended_at: None,
        events: Vec::new(),
        dominant_app: None,
        topics: Vec::new(),
        status: SessionStatus::Active,
    }
}

fn build_summary(event: &RawEvent) -> String {
    match &event.event_type {
        EventType::WindowFocus => format!(
            "{} — {}",
            event.app.as_deref().unwrap_or("?"),
            event.title.as_deref().unwrap_or("?")
        ),
        EventType::ClipboardChange => {
            let content = event.content.as_deref().unwrap_or("");
            let preview: String = content.chars().take(40).collect();
            format!("Copied: {}", preview)
        }
        EventType::FileAccess | EventType::FileEdit => {
            let path = event.content.as_deref().unwrap_or("?");
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string());
            format!("{:?} {}", event.event_type, name)
        }
        EventType::TypingBurst => {
            let meta = event.metadata.as_deref().unwrap_or("");
            let keys = parse_meta_val(meta, "keystrokes").unwrap_or("?");
            let dur = parse_meta_val(meta, "duration").unwrap_or("?s");
            format!(
                "Typed {} keystrokes ({}) in {} — {}",
                keys, dur,
                event.app.as_deref().unwrap_or("?"),
                event.title.as_deref().unwrap_or("?")
            )
        }
        EventType::BrowserNavigation => {
            format!(
                "Navigated to {}",
                event.content.as_deref().unwrap_or("?")
            )
        }
        EventType::Idle => "Idle".into(),
    }
}

fn parse_meta_val<'a>(meta: &'a str, key: &str) -> Option<&'a str> {
    meta.split_whitespace()
        .find(|kv| kv.starts_with(key))
        .and_then(|kv| kv.splitn(2, '=').nth(1))
}
