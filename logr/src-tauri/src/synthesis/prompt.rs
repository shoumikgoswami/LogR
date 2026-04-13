/// Shared prompt builder used by both Ollama and OpenRouter synthesizers.

use chrono::Utc;
use crate::session::types::{EventType, Session};

pub fn build_prompt(session: &Session) -> String {
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
                let mut line = format!("{} [{}] {} — {} ({}s)", ts, context_type, app, doc, dwell);
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

    if lines.is_empty() { "(no events)".into() } else { lines.join("\n") }
}

pub fn parse_dwell(meta: &str) -> u64 {
    for part in meta.split_whitespace() {
        if let Some(val) = part.strip_prefix("dwell=") {
            let digits: String = val.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(n) = digits.parse::<u64>() { return n; }
        }
    }
    0
}

pub fn parse_meta_value<'a>(meta: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{}=", key);
    for part in meta.split_whitespace() {
        if let Some(val) = part.strip_prefix(prefix.as_str()) {
            return Some(val);
        }
    }
    None
}
