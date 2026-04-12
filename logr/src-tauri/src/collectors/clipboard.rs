use async_trait::async_trait;
use arboard::Clipboard;
use chrono::Utc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time;

use super::{Collector, RawEvent};
use crate::session::types::EventType;

pub struct ClipboardCollector {
    pub poll_interval_secs: u64,
    pub max_content_length: usize,
}

impl ClipboardCollector {
    pub fn new() -> Self {
        Self {
            poll_interval_secs: 3,
            max_content_length: 500,
        }
    }
}

#[async_trait]
impl Collector for ClipboardCollector {
    fn name(&self) -> &str {
        "clipboard"
    }

    async fn start(&self, tx: mpsc::Sender<RawEvent>) {
        let poll_interval = Duration::from_secs(self.poll_interval_secs);
        let max_len = self.max_content_length;
        let mut interval = time::interval(poll_interval);
        let mut last_content: Option<String> = None;

        loop {
            interval.tick().await;

            let result = tokio::task::spawn_blocking(move || {
                let mut cb = Clipboard::new()?;
                cb.get_text()
            })
            .await;

            let text = match result {
                Ok(Ok(t)) => t,
                _ => continue,
            };

            if text.trim().is_empty() {
                continue;
            }
            if last_content.as_deref() == Some(&text) {
                continue;
            }

            last_content = Some(text.clone());

            let content = if text.len() > max_len {
                text[..max_len].to_string()
            } else {
                text.clone()
            };

            let content_type = classify_clipboard(&text);

            let event = RawEvent {
                timestamp: Utc::now(),
                event_type: EventType::ClipboardChange,
                app: None,
                title: None,
                content: Some(content),
                context: None,
                metadata: Some(format!("type={}", content_type)),
            };

            if tx.send(event).await.is_err() {
                break;
            }
        }
    }
}

// ── Clipboard content classification ─────────────────────────────────────────

/// Returns a short label describing the type of content copied.
fn classify_clipboard(text: &str) -> &'static str {
    let trimmed = text.trim();
    let lines: Vec<&str> = trimmed.lines().collect();
    let first = lines.first().copied().unwrap_or("").trim();

    // Shell command — starts with common shell prefixes or CLI patterns
    if first.starts_with("$ ")
        || first.starts_with("# ")
        || first.starts_with("> ")
        || is_shell_command(first)
    {
        return "command";
    }

    // File path — looks like an absolute path
    if (first.starts_with('/') || first.starts_with('~') || looks_like_windows_path(first))
        && !first.contains(' ')
    {
        return "filepath";
    }

    // Code — heuristic: has code-like punctuation density or known keywords
    if is_likely_code(trimmed) {
        return "code";
    }

    // Multi-line → probably a block of text or code
    if lines.len() > 3 {
        return "multiline-text";
    }

    "text"
}

fn is_shell_command(s: &str) -> bool {
    let commands = [
        "git ", "npm ", "npx ", "yarn ", "cargo ", "python ", "python3 ",
        "pip ", "docker ", "kubectl ", "curl ", "wget ", "ssh ", "cd ",
        "ls ", "mv ", "cp ", "rm ", "mkdir ", "cat ", "grep ", "awk ",
        "sed ", "chmod ", "sudo ", "brew ", "apt ", "echo ", "export ",
    ];
    commands.iter().any(|c| s.starts_with(c))
}

fn looks_like_windows_path(s: &str) -> bool {
    // e.g. "C:\Users\..." or "D:\"
    s.len() > 3
        && s.chars().nth(1).map(|c| c == ':').unwrap_or(false)
        && s.chars().nth(2).map(|c| c == '\\' || c == '/').unwrap_or(false)
}

fn is_likely_code(text: &str) -> bool {
    // Count code-like characters
    let code_chars = text.chars().filter(|c| matches!(c, '{' | '}' | '(' | ')' | ';' | '=' | '<' | '>')).count();
    let total = text.len().max(1);
    if code_chars * 10 > total {
        return true;
    }

    // Starts with common code keywords
    let first_line = text.lines().next().unwrap_or("").trim();
    let keywords = [
        "fn ", "pub ", "let ", "const ", "var ", "function ", "async ",
        "import ", "export ", "class ", "def ", "if ", "return ",
        "struct ", "enum ", "impl ", "use ", "from ", "type ", "#[",
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE ",
    ];
    keywords.iter().any(|k| first_line.starts_with(k))
}
