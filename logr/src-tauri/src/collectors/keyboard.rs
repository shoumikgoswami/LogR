/// Keyboard activity collector — detects typing bursts without capturing key content.
///
/// Privacy guarantees:
///  - Key identities are NEVER stored or transmitted.
///  - Only keystroke *count* and *duration* are recorded.
///  - Events from blocked apps are filtered downstream by prefilter.
///
/// A "typing burst" is: ≥15 keystrokes, followed by 5 s of silence.

use chrono::Utc;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use crate::session::types::{EventType, RawEvent};

const SETTLE_SECS: u64 = 5;   // seconds of silence before emitting the burst
const MIN_KEYSTROKES: u32 = 15; // minimum keystrokes to bother emitting

pub struct KeyboardCollector;

impl KeyboardCollector {
    pub fn new() -> Self {
        Self
    }

    pub async fn start(&self, tx: mpsc::Sender<RawEvent>) {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_rdev = counter.clone();

        // rdev::listen blocks the calling thread. On macOS, rdev uses CGEventTap
        // which can cause a hard SIGSEGV crash when Accessibility permissions are
        // not granted — this cannot be caught with catch_unwind. Skip keyboard
        // collection on macOS to avoid crashing the app; all other collectors
        // (window, clipboard, filesystem) still function normally.
        #[cfg(not(target_os = "macos"))]
        std::thread::Builder::new()
            .name("logr-keyboard-hook".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = rdev::listen(move |event| {
                        if matches!(event.event_type, rdev::EventType::KeyPress(_)) {
                            counter_rdev.fetch_add(1, Ordering::Relaxed);
                        }
                    });
                }));
                if result.is_err() {
                    tracing::warn!(
                        "[keyboard] rdev hook failed — keyboard collection disabled."
                    );
                }
            })
            .ok();

        #[cfg(target_os = "macos")]
        {
            tracing::info!(
                "[keyboard] Keyboard hook disabled on macOS (requires Accessibility \
                 permissions which may cause crashes). Keystroke counting unavailable."
            );
            drop(counter_rdev); // silence unused warning
        }

        let mut poll_tick = tokio::time::interval(Duration::from_secs(SETTLE_SECS));
        poll_tick.tick().await; // skip first immediate tick

        let mut last_total: u32 = 0;
        let mut burst_start: Option<Instant> = None;
        let mut burst_keystrokes: u32 = 0;

        loop {
            poll_tick.tick().await;

            let total = counter.load(Ordering::Relaxed);
            let delta = total.saturating_sub(last_total);
            last_total = total;

            if delta > 0 {
                // User is still typing — accumulate
                if burst_start.is_none() {
                    burst_start = Some(Instant::now());
                    tracing::debug!("[keyboard] burst started");
                }
                burst_keystrokes += delta;
            } else if burst_keystrokes >= MIN_KEYSTROKES {
                // Silence after a burst — emit event
                let duration_secs = burst_start
                    .map(|s| s.elapsed().as_secs())
                    .unwrap_or(0);

                tracing::debug!(
                    "[keyboard] burst ended: {} keystrokes over ~{}s",
                    burst_keystrokes, duration_secs
                );

                // Capture which window the user was in at burst-end
                let win = tokio::task::spawn_blocking(active_win_pos_rs::get_active_window)
                    .await
                    .ok()
                    .and_then(|r| r.ok());

                let event = RawEvent {
                    timestamp: Utc::now(),
                    event_type: EventType::TypingBurst,
                    app: win.as_ref().map(|w| w.app_name.clone()),
                    title: win.as_ref().map(|w| w.title.clone()),
                    content: None,
                    context: None,
                    metadata: Some(format!(
                        "keystrokes={} duration={}s",
                        burst_keystrokes, duration_secs
                    )),
                };

                if tx.send(event).await.is_err() {
                    return;
                }

                burst_start = None;
                burst_keystrokes = 0;
            } else {
                // Silence with no burst — reset
                burst_start = None;
                burst_keystrokes = 0;
            }
        }
    }
}
