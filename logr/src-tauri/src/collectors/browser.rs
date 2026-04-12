/// Browser navigation collector — detects URL changes within the same browser window.
///
/// The window collector catches tab changes via title changes (3 s dwell).
/// This collector fills the gap: SPA navigation and link clicks where the
/// page title stays the same but the URL changes (e.g. GitHub repo browsing,
/// Twitter/X feed, YouTube playlist).
///
/// Polls the active browser's URL every 5 s via UI Automation.
/// Only emits if URL changed AND app is a browser.

use chrono::Utc;
use std::time::Duration;
use tokio::sync::mpsc;

use super::{context::get_app_context, RawEvent};
use crate::session::types::EventType;

const POLL_SECS: u64 = 5;

pub struct BrowserNavigationCollector;

impl BrowserNavigationCollector {
    pub fn new() -> Self {
        Self
    }

    pub async fn start(&self, tx: mpsc::Sender<RawEvent>) {
        let mut interval = tokio::time::interval(Duration::from_secs(POLL_SECS));
        interval.tick().await;

        let mut last_url: Option<String> = None;
        let mut last_app: Option<String> = None;

        loop {
            interval.tick().await;

            // Get active window
            let win = match tokio::task::spawn_blocking(active_win_pos_rs::get_active_window)
                .await
                .ok()
                .and_then(|r| r.ok())
            {
                Some(w) => w,
                None => continue,
            };

            let app_lower = win.app_name.to_lowercase();
            if !is_browser(&app_lower) {
                // Not a browser — reset and skip
                last_url = None;
                last_app = None;
                continue;
            }

            let pid = win.process_id;
            let app_name = win.app_name.clone();
            let title = win.title.clone();

            // Fetch current URL via UIAutomation (blocking)
            let url = tokio::task::spawn_blocking(move || get_app_context(pid, &app_name.to_lowercase()))
                .await
                .ok()
                .flatten();

            let url = match url {
                Some(u) if u.starts_with("http") => u,
                _ => {
                    last_url = None;
                    continue;
                }
            };

            // App changed — reset baseline
            if last_app.as_deref() != Some(&win.app_name) {
                last_app = Some(win.app_name.clone());
                last_url = Some(url);
                continue;
            }

            // Same app, different URL → emit
            if last_url.as_deref() != Some(&url) {
                tracing::debug!("[browser] URL changed: {} → {}", last_url.as_deref().unwrap_or("(none)"), url);

                let event = RawEvent {
                    timestamp: Utc::now(),
                    event_type: EventType::BrowserNavigation,
                    app: Some(win.app_name.clone()),
                    title: Some(title),
                    content: Some(url.clone()),
                    context: None,
                    metadata: Some(format!("source=url_poll")),
                };

                if tx.send(event).await.is_err() {
                    return;
                }

                last_url = Some(url);
            }
        }
    }
}

fn is_browser(a: &str) -> bool {
    ["chrome", "firefox", "safari", "arc", "brave", "edge", "opera"]
        .iter()
        .any(|e| a.contains(e))
}
