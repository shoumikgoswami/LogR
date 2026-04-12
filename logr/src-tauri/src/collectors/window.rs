use async_trait::async_trait;
use chrono::Utc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time;

use super::{context::get_app_context, screenshot::describe_screen, Collector, RawEvent};
use crate::session::types::EventType;

/// How long a title must be visible before we emit an event for it.
const TITLE_MIN_DWELL_SECS: u64 = 3;

/// Take a periodic snapshot while in the same window this often.
const PERIODIC_SNAPSHOT_SECS: u64 = 45;

pub struct WindowCollector {
    pub poll_interval_ms: u64,
    pub min_dwell_secs: u64,
    /// Ollama base URL — used for vision screenshot description.
    pub ollama_url: String,
    /// Vision model name. Empty string = vision disabled.
    pub vision_model: String,
}

impl WindowCollector {
    pub fn new() -> Self {
        Self {
            poll_interval_ms: 2000,
            min_dwell_secs: 10,
            ollama_url: "http://localhost:11434".into(),
            vision_model: String::new(),
        }
    }
}

struct WindowState {
    app: String,
    title: String,
    process_id: u64,
    focused_at: Instant,
    /// When we last emitted an event for this window (for periodic snapshots).
    last_emitted_at: Instant,
    /// Async task that will resolve to a vision description.
    vision_handle: Option<JoinHandle<Option<String>>>,
}

#[async_trait]
impl Collector for WindowCollector {
    fn name(&self) -> &str {
        "window"
    }

    async fn start(&self, tx: mpsc::Sender<RawEvent>) {
        let poll_interval = Duration::from_millis(self.poll_interval_ms);
        let min_dwell = Duration::from_secs(self.min_dwell_secs);
        let title_min_dwell = Duration::from_secs(TITLE_MIN_DWELL_SECS);
        let periodic = Duration::from_secs(PERIODIC_SNAPSHOT_SECS);

        let mut interval = time::interval(poll_interval);
        let mut last_window: Option<WindowState> = None;

        let ollama_url = self.ollama_url.clone();
        let vision_model = self.vision_model.clone();

        loop {
            interval.tick().await;

            let win_result =
                tokio::task::spawn_blocking(active_win_pos_rs::get_active_window).await;
            let current = match win_result {
                Ok(Ok(w)) => w,
                _ => continue,
            };

            let app = current.app_name.clone();
            let title = current.title.clone();
            let process_id = current.process_id;

            match last_window.take() {
                None => {
                    // First observation — start tracking + kick off vision
                    let handle = spawn_vision(&ollama_url, &vision_model);
                    last_window = Some(WindowState {
                        app,
                        title,
                        process_id,
                        focused_at: Instant::now(),
                        last_emitted_at: Instant::now(),
                        vision_handle: handle,
                    });
                }

                Some(mut prev) => {
                    let same_app = prev.app == app;
                    let same_title = prev.title == title;

                    if same_app && same_title {
                        // ── Periodic snapshot while in the same window ────────────
                        if prev.last_emitted_at.elapsed() >= periodic {
                            tracing::debug!("[window] periodic snapshot for '{}'", prev.title);
                            let vision_desc = collect_vision(
                                prev.vision_handle.take(),
                                &prev.title,
                            ).await;

                            // Kick off a fresh vision handle for next period
                            prev.vision_handle = spawn_vision(&ollama_url, &vision_model);
                            prev.last_emitted_at = Instant::now();

                            let acc = collect_acc(prev.process_id, &prev.app).await;
                            if vision_desc.is_some() || acc.is_some() {
                                let had_vision = vision_desc.is_some();
                                let context = merge_context(vision_desc, acc);
                                let dwell = prev.focused_at.elapsed();
                                let ctx = parse_window_context(&prev.app, &prev.title);
                                let event = make_event(
                                    &prev.app, &prev.title, ctx, context,
                                    dwell, had_vision, "periodic",
                                );
                                if tx.send(event).await.is_err() { return; }
                            }
                        }
                        last_window = Some(prev);
                        continue;
                    }

                    // ── Title changed (same app — e.g. browser tab / editor file) ──
                    let dwell = prev.focused_at.elapsed();
                    let effective_min = if same_app { title_min_dwell } else { min_dwell };

                    tracing::debug!(
                        "[window] change: '{}' → '{}' | dwell={}s | same_app={}",
                        prev.title, title, dwell.as_secs(), same_app
                    );

                    if dwell >= effective_min {
                        let vision_desc = collect_vision(prev.vision_handle.take(), &prev.title).await;
                        let acc = collect_acc(prev.process_id, &prev.app).await;
                        let had_vision = vision_desc.is_some();
                        let context = merge_context(vision_desc, acc);
                        let ctx = parse_window_context(&prev.app, &prev.title);
                        let kind = if same_app { "title_change" } else { "app_switch" };
                        let event = make_event(
                            &prev.app, &prev.title, ctx, context,
                            dwell, had_vision, kind,
                        );
                        if tx.send(event).await.is_err() { return; }
                    } else if let Some(handle) = prev.vision_handle {
                        handle.abort();
                    }

                    // Start tracking new window
                    let handle = spawn_vision(&ollama_url, &vision_model);
                    last_window = Some(WindowState {
                        app,
                        title,
                        process_id,
                        focused_at: Instant::now(),
                        last_emitted_at: Instant::now(),
                        vision_handle: handle,
                    });
                }
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn make_event(
    app: &str,
    title: &str,
    ctx: WindowContext,
    context: Option<String>,
    dwell: Duration,
    had_vision: bool,
    kind: &str,
) -> RawEvent {
    RawEvent {
        timestamp: Utc::now(),
        event_type: EventType::WindowFocus,
        app: Some(app.to_string()),
        title: Some(title.to_string()),
        content: ctx.document,
        context,
        metadata: Some(format!(
            "dwell={}s context_type={} vision={} kind={}",
            dwell.as_secs(),
            ctx.context_type,
            had_vision,
            kind,
        )),
    }
}

fn merge_context(vision: Option<String>, acc: Option<String>) -> Option<String> {
    match (vision, acc) {
        (Some(v), Some(a)) => Some(format!("{}\n[{}]", v, a)),
        (Some(v), None) => Some(v),
        (None, Some(a)) => Some(a),
        (None, None) => None,
    }
}

async fn collect_vision(
    handle: Option<JoinHandle<Option<String>>>,
    title: &str,
) -> Option<String> {
    let handle = handle?;
    tracing::debug!("[window] awaiting vision handle for '{}'", title);
    match tokio::time::timeout(Duration::from_secs(30), handle).await {
        Ok(Ok(Some(desc))) => {
            tracing::debug!("[window] vision ready for '{}'", title);
            Some(desc)
        }
        Ok(Ok(None)) => {
            tracing::debug!("[window] vision returned None for '{}'", title);
            None
        }
        Ok(Err(e)) => {
            tracing::warn!("[window] vision task panicked for '{}': {}", title, e);
            None
        }
        Err(_) => {
            tracing::warn!("[window] vision timed out (30s) for '{}'", title);
            None
        }
    }
}

async fn collect_acc(pid: u64, app: &str) -> Option<String> {
    let app_lower = app.to_lowercase();
    tokio::task::spawn_blocking(move || get_app_context(pid, &app_lower))
        .await
        .ok()
        .flatten()
}

/// Spawns a vision task, re-reading config so model changes in Settings take
/// effect immediately without restarting the app.
fn spawn_vision(_ollama_url: &str, _vision_model: &str) -> Option<JoinHandle<Option<String>>> {
    // Always read the latest config so Settings changes are picked up live.
    let config = crate::commands::load_config_sync();
    let model = config.vision_model;
    if model.trim().is_empty() {
        return None;
    }
    let url = config.ollama_url;
    Some(tokio::spawn(async move {
        describe_screen(&url, &model).await
    }))
}

// ── Window title parsing ──────────────────────────────────────────────────────

struct WindowContext {
    document: Option<String>,
    context_type: &'static str,
}

fn parse_window_context(app: &str, title: &str) -> WindowContext {
    let app_lower = app.to_lowercase();

    if is_editor(&app_lower) {
        let cleaned = strip_app_suffix(title, app);
        let parts: Vec<&str> = cleaned
            .split(&['-', '—'][..])
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let document = if parts.len() >= 2 {
            Some(format!("{} (in {})", parts[0], parts[1]))
        } else if parts.len() == 1 {
            Some(parts[0].to_string())
        } else {
            Some(cleaned.to_string())
        };
        return WindowContext { document, context_type: "editor" };
    }

    if is_browser(&app_lower) {
        let without_browser = strip_app_suffix(title, app);
        let parts: Vec<&str> = without_browser
            .split(" - ")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        let document = if parts.len() > 1 {
            Some(format!("{} — {}", parts[0], parts[parts.len() - 1]))
        } else {
            Some(without_browser.trim().to_string())
        };
        return WindowContext { document, context_type: "browser" };
    }

    if is_terminal(&app_lower) {
        let cleaned = strip_app_suffix(title, app).trim().to_string();
        let document = if cleaned.is_empty() { None } else { Some(cleaned) };
        return WindowContext { document, context_type: "terminal" };
    }

    let cleaned = strip_app_suffix(title, app).trim().to_string();
    let document = if cleaned.is_empty() || cleaned == title.trim() {
        None
    } else {
        Some(cleaned)
    };
    WindowContext { document, context_type: "other" }
}

fn strip_app_suffix<'a>(title: &'a str, app: &str) -> &'a str {
    let suffixes: &[&str] = &[
        " - Visual Studio Code", " — Visual Studio Code",
        " - Code", " - Cursor", " — Cursor",
        " - Google Chrome", " — Google Chrome",
        " - Mozilla Firefox", " — Mozilla Firefox",
        " - Microsoft Edge", " — Microsoft Edge",
        " - Arc", " — Arc", " - Brave", " - Safari",
        " - Windows Terminal", " — Windows Terminal",
        " - Terminal", " — Terminal", " - iTerm2", " - Warp",
        " - IntelliJ IDEA", " - PyCharm", " - WebStorm",
        " - RustRover", " - Rider",
    ];
    let mut result = title;
    for suffix in suffixes {
        if let Some(s) = result.strip_suffix(suffix) { return s; }
    }
    let app_suffix = format!(" - {}", app);
    let app_suffix2 = format!(" — {}", app);
    if let Some(s) = result.strip_suffix(&app_suffix) { result = s; }
    else if let Some(s) = result.strip_suffix(&app_suffix2) { result = s; }
    result
}

fn is_editor(a: &str) -> bool {
    ["code","cursor","zed","xcode","intellij","rider","goland","pycharm","webstorm",
     "clion","rustrover","sublime","neovim","nvim","vim","emacs","helix","notepad","atom"]
        .iter().any(|e| a.contains(e))
}
fn is_browser(a: &str) -> bool {
    ["chrome","firefox","safari","arc","brave","edge","opera"].iter().any(|e| a.contains(e))
}
fn is_terminal(a: &str) -> bool {
    ["terminal","iterm","warp","kitty","alacritty","hyper","cmd","powershell","wezterm","conhost"]
        .iter().any(|e| a.contains(e))
}
