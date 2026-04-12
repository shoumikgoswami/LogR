use std::path::{Path, PathBuf};

use chrono::Local;

use crate::session::{
    classify_app,
    types::{EventType, GeneratedNote, RawEvent, Session},
};

pub struct MarkdownWriter {
    pub notes_dir: PathBuf,
}

impl MarkdownWriter {
    pub fn new(notes_dir: PathBuf) -> Self {
        Self { notes_dir }
    }

    /// Write a note with LLM-generated markdown body.
    pub fn write(&self, session: &Session, markdown: &str) -> Result<GeneratedNote, String> {
        self.write_inner(session, markdown, false)
    }

    /// Write a note using raw session data — no LLM required.
    pub fn write_raw(&self, session: &Session) -> Result<GeneratedNote, String> {
        let markdown = build_raw_markdown(session);
        self.write_inner(session, &markdown, true)
    }

    fn write_inner(
        &self,
        session: &Session,
        body: &str,
        raw: bool,
    ) -> Result<GeneratedNote, String> {
        let now = Local::now();
        let date_dir = self.notes_dir.join(now.format("%Y-%m-%d").to_string());
        std::fs::create_dir_all(&date_dir).map_err(|e| e.to_string())?;

        let filename = build_filename(session, &now, raw);
        let note_path = date_dir.join(&filename);

        let frontmatter = build_frontmatter(session, &now, raw);
        let full_content = format!("{}\n{}", frontmatter, body);

        std::fs::write(&note_path, &full_content).map_err(|e| e.to_string())?;

        tracing::info!("Note written: {:?}", note_path);

        self.update_index(session, &now)?;

        Ok(GeneratedNote {
            session_id: session.id.clone(),
            content: full_content,
            file_path: note_path.to_string_lossy().into_owned(),
            created_at: chrono::Utc::now(),
        })
    }

    fn update_index(&self, session: &Session, now: &chrono::DateTime<Local>) -> Result<(), String> {
        let index_path = self.notes_dir.join("index.md");
        let date_str = now.format("%Y-%m-%d").to_string();

        let start = session
            .started_at
            .with_timezone(&Local)
            .format("%H:%M")
            .to_string();
        let duration_mins = session
            .ended_at
            .map(|e| (e - session.started_at).num_minutes())
            .unwrap_or(0);

        let category = classify_app(session.dominant_app.as_deref().unwrap_or(""));
        let app = session.dominant_app.as_deref().unwrap_or("Unknown");
        let topics_preview = session.topics.first().cloned().unwrap_or_default();

        let new_row = format!(
            "| {} | {:?} — {} — {} | {} min |\n",
            start, category, app, topics_preview, duration_mins
        );

        let existing = std::fs::read_to_string(&index_path).unwrap_or_default();
        let header = format!(
            "# LogR — {}\n\n| Time | Activity | Duration |\n|------|----------|----------|\n",
            date_str
        );
        let content = if existing.is_empty() {
            format!("{}{}", header, new_row)
        } else {
            format!("{}{}", existing, new_row)
        };

        std::fs::write(&index_path, content).map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn build_filename(session: &Session, now: &chrono::DateTime<Local>, raw: bool) -> String {
    let time_part = now.format("%H-%M").to_string();
    let category = classify_app(session.dominant_app.as_deref().unwrap_or(""));
    let app_slug = session
        .dominant_app
        .as_deref()
        .unwrap_or("unknown")
        .to_lowercase()
        .replace(' ', "_")
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_')
        .take(20)
        .collect::<String>();
    let suffix = if raw { "_raw" } else { "" };
    format!(
        "{}_{}_{}{}.md",
        time_part,
        format!("{:?}", category).to_lowercase(),
        app_slug,
        suffix
    )
}

fn build_frontmatter(
    session: &Session,
    now: &chrono::DateTime<Local>,
    raw: bool,
) -> String {
    let start = session
        .started_at
        .with_timezone(&Local)
        .format("%H:%M")
        .to_string();
    let end = session
        .ended_at
        .map(|e| e.with_timezone(&Local).format("%H:%M").to_string())
        .unwrap_or_else(|| now.format("%H:%M").to_string());
    let duration = session
        .ended_at
        .map(|e| (e - session.started_at).num_minutes())
        .unwrap_or(0);
    let category = classify_app(session.dominant_app.as_deref().unwrap_or(""));
    let topics_yaml = session
        .topics
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "'")))
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "---\ndate: {}\ntime: {} – {}\napp: {}\ncategory: {:?}\ntopics: [{}]\nduration_mins: {}\nevent_count: {}\nsynthesized: {}\n---\n\n",
        now.format("%Y-%m-%d"),
        start,
        end,
        session.dominant_app.as_deref().unwrap_or("Unknown"),
        category,
        topics_yaml,
        duration,
        session.events.len(),
        !raw,
    )
}

/// Produce readable markdown directly from raw session events — no LLM needed.
fn build_raw_markdown(session: &Session) -> String {
    use std::collections::HashMap;

    let category = classify_app(session.dominant_app.as_deref().unwrap_or(""));
    let app = session.dominant_app.as_deref().unwrap_or("Unknown");
    let duration = session
        .ended_at
        .map(|e| (e - session.started_at).num_minutes())
        .unwrap_or(0);

    let mut lines: Vec<String> = Vec::new();

    // ── Heading ──────────────────────────────────────────────────
    lines.push(format!(
        "**{:?} session — {}** ({} min)",
        category, app, duration
    ));
    lines.push(String::new());

    if !session.topics.is_empty() {
        lines.push(format!("**Topics:** {}", session.topics.join(", ")));
        lines.push(String::new());
    }

    // ── Apps seen ─────────────────────────────────────────────────
    // Collect unique (app, title) pairs from WindowFocus events
    let mut apps_seen: Vec<(String, String)> = Vec::new();
    let mut seen_titles: std::collections::HashSet<String> = Default::default();
    for event in &session.events {
        if matches!(event.event_type, EventType::WindowFocus) {
            let a = event.app.as_deref().unwrap_or("Unknown").to_string();
            let t = event.title.as_deref().unwrap_or("").to_string();
            let key = format!("{}\x00{}", a, t);
            if !t.is_empty() && seen_titles.insert(key) {
                apps_seen.push((a, t));
            }
        }
    }
    if !apps_seen.is_empty() {
        lines.push("**Applications & windows:**".into());
        // Group titles by app
        let mut by_app: HashMap<String, Vec<String>> = HashMap::new();
        for (a, t) in &apps_seen {
            by_app.entry(a.clone()).or_default().push(t.clone());
        }
        // Emit in order of first appearance
        let mut emitted_apps: Vec<String> = Vec::new();
        for (a, _) in &apps_seen {
            if !emitted_apps.contains(a) {
                emitted_apps.push(a.clone());
            }
        }
        for app_name in &emitted_apps {
            if let Some(titles) = by_app.get(app_name) {
                lines.push(format!("- **{}**", app_name));
                for t in titles {
                    lines.push(format!("  - {}", t));
                }
            }
        }
        lines.push(String::new());
    }

    // ── Files edited/accessed ─────────────────────────────────────
    let file_edits: Vec<&RawEvent> = session
        .events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::FileEdit))
        .collect();
    let file_accesses: Vec<&RawEvent> = session
        .events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::FileAccess))
        .collect();

    if !file_edits.is_empty() {
        lines.push("**Files edited:**".into());
        // Deduplicate, show parent dir + filename
        let mut seen: std::collections::HashSet<String> = Default::default();
        for event in &file_edits {
            let path_str = event.content.as_deref().unwrap_or("?");
            let p = Path::new(path_str);
            let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path_str.to_string());
            let dir = p.parent().and_then(|d| d.file_name()).map(|d| format!("{}/", d.to_string_lossy())).unwrap_or_default();
            let display = format!("{}{}", dir, name);
            if seen.insert(display.clone()) {
                lines.push(format!(
                    "- `{}` at {}",
                    display,
                    event.timestamp.format("%H:%M")
                ));
            }
        }
        lines.push(String::new());
    }

    if !file_accesses.is_empty() {
        lines.push("**Files opened:**".into());
        let mut seen: std::collections::HashSet<String> = Default::default();
        for event in &file_accesses {
            let path_str = event.content.as_deref().unwrap_or("?");
            let name = Path::new(path_str)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path_str.to_string());
            if seen.insert(name.clone()) {
                lines.push(format!("- `{}`", name));
            }
        }
        lines.push(String::new());
    }

    // ── Clipboard snippets ────────────────────────────────────────
    let clipboard_events: Vec<&RawEvent> = session
        .events
        .iter()
        .filter(|e| matches!(e.event_type, EventType::ClipboardChange))
        .collect();

    if !clipboard_events.is_empty() {
        lines.push("**Clipboard activity:**".into());
        for event in &clipboard_events {
            let content = event.content.as_deref().unwrap_or("");
            let preview: String = content.chars().take(100).collect();
            let ellipsis = if content.len() > 100 { "…" } else { "" };
            lines.push(format!(
                "- [{}] `{}{}`",
                event.timestamp.format("%H:%M"),
                preview,
                ellipsis
            ));
        }
        lines.push(String::new());
    }

    // ── Chronological event log ───────────────────────────────────
    lines.push("**Event log:**".into());
    for event in &session.events {
        let ts = event.timestamp.format("%H:%M:%S");
        let meta = event.metadata.as_deref().unwrap_or("");
        let entry = match &event.event_type {
            EventType::WindowFocus => {
                let app = event.app.as_deref().unwrap_or("?");
                let dwell = parse_dwell_secs(meta);
                let dwell_label = if dwell >= 60 {
                    format!("{}m{}s", dwell / 60, dwell % 60)
                } else {
                    format!("{}s", dwell)
                };
                let context_type = parse_meta_val(meta, "context_type").unwrap_or("app");
                let detail = event.content.as_deref().or(event.title.as_deref()).unwrap_or("(no title)");
                let mut line = format!("- `{}` **{}** [{}, {}] — {}", ts, app, context_type, dwell_label, detail);
                if let Some(ctx) = &event.context {
                    line.push_str(&format!("\n  - → {}", ctx));
                }
                line
            }
            EventType::ClipboardChange => {
                let content_type = parse_meta_val(meta, "type").unwrap_or("text");
                let raw = event.content.as_deref().unwrap_or("");
                let preview: String = raw.chars().take(100).collect();
                let ellipsis = if raw.len() > 100 { "…" } else { "" };
                format!("- `{}` Clipboard [{}]: `{}{}`", ts, content_type, preview, ellipsis)
            }
            EventType::FileEdit => {
                let path_str = event.content.as_deref().unwrap_or("?");
                let p = Path::new(path_str);
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path_str.to_string());
                let dir = p.parent().and_then(|d| d.file_name()).map(|n| format!("{}/", n.to_string_lossy())).unwrap_or_default();
                let ext = p.extension().map(|e| format!(" ({})", e.to_string_lossy())).unwrap_or_default();
                let mut line = format!("- `{}` Edited `{}{}`{}", ts, dir, name, ext);
                if let Some(snippet) = &event.context {
                    let preview: String = snippet.chars().take(200).collect();
                    let ellipsis = if snippet.len() > 200 { "…" } else { "" };
                    line.push_str(&format!("\n  ```\n  {}{}\n  ```", preview, ellipsis));
                }
                line
            }
            EventType::FileAccess => {
                let path_str = event.content.as_deref().unwrap_or("?");
                let p = Path::new(path_str);
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path_str.to_string());
                let dir = p.parent().and_then(|d| d.file_name()).map(|n| format!("{}/", n.to_string_lossy())).unwrap_or_default();
                format!("- `{}` Opened `{}{}`", ts, dir, name)
            }
            EventType::TypingBurst => {
                let keys = parse_meta_val(meta, "keystrokes").unwrap_or("?");
                let dur = parse_meta_val(meta, "duration").unwrap_or("?");
                let app = event.app.as_deref().unwrap_or("?");
                let title = event.title.as_deref().unwrap_or("?");
                format!("- `{}` Typed {} keystrokes ({}s) in **{}** — {}", ts, keys, dur, app, title)
            }
            EventType::BrowserNavigation => {
                let url = event.content.as_deref().unwrap_or("?");
                let title = event.title.as_deref().unwrap_or("?");
                format!("- `{}` Navigated → [{}]({})", ts, title, url)
            }
            EventType::Idle => continue,
        };
        lines.push(entry);
    }

    lines.push(String::new());
    lines.push(format!(
        "*Raw note — {} events captured. AI summary unavailable at write time.*",
        session.events.len()
    ));

    lines.join("\n")
}

fn parse_dwell_secs(meta: &str) -> u64 {
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

fn parse_meta_val<'a>(meta: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    for part in meta.split_whitespace() {
        if let Some(val) = part.strip_prefix(prefix.as_str()) {
            return Some(val);
        }
    }
    None
}
