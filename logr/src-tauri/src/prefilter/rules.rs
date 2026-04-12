use std::sync::OnceLock;

use regex::Regex;

use crate::session::types::{EventType, RawEvent};

pub enum FilterResult {
    Allow,
    Drop(String),
}

// ──────────────────────────────────────────
// Blocklists
// ──────────────────────────────────────────

const BLOCKED_APPS: &[&str] = &[
    "Finder",
    "Explorer",
    "Dock",
    "Spotify",
    "Apple Music",
    "VLC",
    "Netflix",
    "YouTube",
    "System Preferences",
    "Settings",
    "Discord",
    "Slack",
    "Mail",
    "Outlook",
    "1Password",
    "Keychain",
    "Calculator",
    "Clock",
    "Photos",
    "Preview",
];

const BLOCKED_TITLE_SUBSTRINGS: &[&str] = &[
    "password",
    "login",
    "sign in",
    "signin",
    "credit card",
    "billing",
    "payment",
    "youtube.com",
    "netflix.com",
    "twitch.tv",
    "pornhub",
    "incognito",
    "new tab",
    "blank page",
];

// ──────────────────────────────────────────
// Clipboard patterns (pre-compiled)
// ──────────────────────────────────────────

fn email_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"@.+\..+").unwrap())
}

fn bare_url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^https?://\S+$").unwrap())
}

fn credit_card_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{4}[\s\-]?\d{4}[\s\-]?\d{4}[\s\-]?\d{4}$").unwrap())
}

fn looks_like_password(s: &str) -> bool {
    // >20 chars with no spaces often indicates a token/secret/password
    s.len() > 20 && !s.contains(' ')
}

// ──────────────────────────────────────────
// Main filter entry point
// ──────────────────────────────────────────

pub fn apply(event: &RawEvent) -> FilterResult {
    match &event.event_type {
        EventType::WindowFocus => filter_window_event(event),
        EventType::ClipboardChange => filter_clipboard_event(event),
        EventType::FileAccess | EventType::FileEdit => filter_file_event(event),
        EventType::TypingBurst => filter_typing_event(event),
        EventType::BrowserNavigation => filter_browser_nav_event(event),
        EventType::Idle => FilterResult::Drop("idle event".into()),
    }
}

fn filter_window_event(event: &RawEvent) -> FilterResult {
    // App blocklist
    if let Some(app) = &event.app {
        let app_lower = app.to_lowercase();
        for blocked in BLOCKED_APPS {
            if app_lower == blocked.to_lowercase() {
                return FilterResult::Drop(format!("blocked app: {}", app));
            }
        }
    }

    // Title blocklist
    if let Some(title) = &event.title {
        let title_lower = title.to_lowercase();
        for substr in BLOCKED_TITLE_SUBSTRINGS {
            if title_lower.contains(substr) {
                return FilterResult::Drop(format!("blocked title keyword: {}", substr));
            }
        }
    }

    FilterResult::Allow
}

fn filter_clipboard_event(event: &RawEvent) -> FilterResult {
    let content = match &event.content {
        Some(c) => c,
        None => return FilterResult::Drop("empty clipboard".into()),
    };

    let trimmed = content.trim();

    // Too short
    if trimmed.len() < 10 {
        return FilterResult::Drop("clipboard content too short".into());
    }

    // Looks like a credential
    if email_re().is_match(trimmed) {
        return FilterResult::Drop("clipboard matches email/credential pattern".into());
    }
    if looks_like_password(trimmed) {
        return FilterResult::Drop("clipboard looks like a password/token".into());
    }

    // Bare URL
    if bare_url_re().is_match(trimmed) {
        return FilterResult::Drop("clipboard is a bare URL".into());
    }

    // Credit card pattern
    if credit_card_re().is_match(trimmed) {
        return FilterResult::Drop("clipboard matches credit card pattern".into());
    }

    // LogR's own output (raw note footer, frontmatter etc.)
    if trimmed.starts_with("*Raw note —") || trimmed.starts_with("---\ndate:") {
        return FilterResult::Drop("clipboard contains LogR internal content".into());
    }

    FilterResult::Allow
}

fn filter_typing_event(event: &RawEvent) -> FilterResult {
    // Drop typing bursts from blocked apps (e.g. password managers)
    if let Some(app) = &event.app {
        let app_lower = app.to_lowercase();
        for blocked in BLOCKED_APPS {
            if app_lower == blocked.to_lowercase() {
                return FilterResult::Drop(format!("blocked app: {}", app));
            }
        }
    }
    FilterResult::Allow
}

fn filter_browser_nav_event(event: &RawEvent) -> FilterResult {
    if let Some(url) = &event.content {
        let lower = url.to_lowercase();
        for substr in BLOCKED_TITLE_SUBSTRINGS {
            if lower.contains(substr) {
                return FilterResult::Drop(format!("blocked URL keyword: {}", substr));
            }
        }
    }
    FilterResult::Allow
}

fn filter_file_event(event: &RawEvent) -> FilterResult {
    // File events have path in content — already filtered by FilesystemCollector
    // but double-check for any hidden paths that slipped through
    if let Some(path) = &event.content {
        if path.contains("/.") || path.contains("\\.") {
            return FilterResult::Drop("hidden file path".into());
        }
    }
    FilterResult::Allow
}
