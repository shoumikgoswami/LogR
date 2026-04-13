pub mod collectors;
pub mod commands;
pub mod prefilter;
pub mod session;
pub mod state;
pub mod synthesis;
pub mod tray;
pub mod writer;

use std::sync::{Arc, Mutex};

use collectors::{
    browser::BrowserNavigationCollector,
    clipboard::ClipboardCollector,
    filesystem::FilesystemCollector,
    keyboard::KeyboardCollector,
    window::WindowCollector,
    Collector,
};
use session::{types::RawEvent, FeedEntry, Session, SessionBuffer};
use state::{FlushHandle, SharedStats};
use synthesis::ollama::{OllamaClient, OllamaConfig};
use synthesis::openrouter::OpenRouterClient;
use tauri::Manager;
use tokio::sync::mpsc;
use tokio::time::{interval, Duration};
use writer::MarkdownWriter;

const IDLE_CHECK_SECS: u64 = 20;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(FlushHandle(Mutex::new(None)))
        .manage(SharedStats::new())
        .invoke_handler(tauri::generate_handler![
            commands::save_config,
            commands::load_config,
            commands::reset_config,
            commands::clear_notes,
            commands::check_ollama,
            commands::list_ollama_models,
            commands::list_openrouter_models,
            commands::get_status,
            commands::flush_session,
            commands::write_test_note,
            commands::test_vision,
            commands::test_vision_with,
            commands::check_openrouter,
        ])
        .setup(|app| {
            tray::setup_tray(&app.handle())?;

            let (flush_tx, flush_rx) = mpsc::channel::<()>(4);
            *app.state::<FlushHandle>().0.lock().unwrap() = Some(flush_tx);

            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                start_pipeline(flush_rx, app_handle).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running LogR");
}

async fn start_pipeline(flush_rx: mpsc::Receiver<()>, app: tauri::AppHandle) {
    let config = commands::load_config_sync();

    let (raw_tx, raw_rx) = mpsc::channel::<RawEvent>(256);
    let (session_tx, session_rx) = mpsc::channel::<Session>(32);
    let (feed_tx, mut feed_rx) = mpsc::channel::<FeedEntry>(256);

    let event_count: Arc<Mutex<usize>> = Arc::new(Mutex::new(0));
    let vision_count: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));

    let tx1 = raw_tx.clone();
    let tx2 = raw_tx.clone();
    let tx3 = raw_tx.clone();
    let tx4 = raw_tx.clone();
    let tx5 = raw_tx.clone();
    drop(raw_tx);

    let dwell = config.min_dwell_secs;
    let ollama_url_for_vision = config.ollama_url.clone();
    let vision_model = config.vision_model.clone();
    tokio::spawn(async move {
        let mut wc = WindowCollector::new();
        wc.min_dwell_secs = dwell;
        wc.poll_interval_ms = 2000;
        wc.ollama_url = ollama_url_for_vision;
        wc.vision_model = vision_model;
        wc.start(tx1).await;
    });
    tokio::spawn(async move { ClipboardCollector::new().start(tx2).await });
    let configured_notes = std::path::PathBuf::from(&config.notes_dir);
    tokio::spawn(async move {
        let mut fc = FilesystemCollector::new();
        if !fc.exclude_dirs.contains(&configured_notes) {
            fc.exclude_dirs.push(configured_notes);
        }
        fc.start(tx3).await;
    });
    tokio::spawn(async move { KeyboardCollector::new().start(tx4).await });
    tokio::spawn(async move { BrowserNavigationCollector::new().start(tx5).await });

    let idle_timeout = config.session_idle_timeout_secs;
    let ec = event_count.clone();
    let vc = vision_count.clone();
    tokio::spawn(async move {
        run_session_buffer(raw_rx, flush_rx, session_tx, feed_tx, idle_timeout, ec, vc).await;
    });

    tokio::spawn(async move {
        while let Some(entry) = feed_rx.recv().await {
            if entry.filtered {
                tracing::debug!("[DROP] {} | {}", entry.summary,
                    entry.filter_reason.as_deref().unwrap_or("?"));
            } else {
                tracing::info!("[FEED] {} | {}", entry.timestamp, entry.summary);
            }
        }
    });

    run_synthesis(session_rx, config, event_count, vision_count, app).await;
}

async fn run_session_buffer(
    mut raw_rx: mpsc::Receiver<RawEvent>,
    mut flush_rx: mpsc::Receiver<()>,
    session_tx: mpsc::Sender<Session>,
    feed_tx: mpsc::Sender<FeedEntry>,
    idle_timeout_secs: u64,
    event_count: Arc<Mutex<usize>>,
    vision_count: Arc<Mutex<u32>>,
) {
    let mut buffer = SessionBuffer::new(session_tx, feed_tx, idle_timeout_secs, event_count, vision_count);
    let mut idle_tick = interval(Duration::from_secs(IDLE_CHECK_SECS));
    idle_tick.tick().await;

    loop {
        tokio::select! {
            Some(event) = raw_rx.recv() => {
                buffer.process(event).await;
            }
            _ = idle_tick.tick() => {
                buffer.check_idle().await;
            }
            Some(()) = flush_rx.recv() => {
                tracing::info!("Manual flush triggered");
                buffer.force_flush().await;
            }
        }
    }
}

async fn run_synthesis(
    mut session_rx: mpsc::Receiver<Session>,
    config: state::DriftlogConfig,
    event_count: Arc<Mutex<usize>>,
    vision_count: Arc<Mutex<u32>>,
    app: tauri::AppHandle,
) {
    let ollama = OllamaClient::new(OllamaConfig {
        model: config.ollama_model.clone(),
        base_url: config.ollama_url.clone(),
        temperature: 0.3,
        max_tokens: 512,
    });

    let notes_dir = std::path::PathBuf::from(&config.notes_dir);
    std::fs::create_dir_all(&notes_dir).ok();
    let writer = MarkdownWriter::new(notes_dir);

    let mut queue: Vec<Session> = Vec::new();

    // Check provider status at startup for dashboard.
    let cfg = commands::load_config_sync();
    if cfg.provider == "openrouter" {
        let or = OpenRouterClient::new(cfg.openrouter_api_key.clone(), cfg.openrouter_model.clone());
        let ok = or.check_status().await;
        push_stats(&app, |s| {
            s.ollama_running = ok;
            s.model_available = ok;
            s.is_watching = true;
        });
    } else {
        let (running, has_model) = ollama.check_status().await;
        push_stats(&app, |s| {
            s.ollama_running = running;
            s.model_available = has_model;
            s.is_watching = true;
        });
    }

    // Status check every 10 s; session drain retry every 60 s.
    let mut status_tick = interval(Duration::from_secs(10));
    let mut drain_tick = interval(Duration::from_secs(60));
    status_tick.tick().await;
    drain_tick.tick().await;

    loop {
        tokio::select! {
            Some(session) = session_rx.recv() => {
                if session.events.is_empty() { continue; }
                tracing::info!(
                    "Session received: {} events, app={:?}, topics={:?}",
                    session.events.len(), session.dominant_app, session.topics
                );
                queue.push(session);
                drain_queue(&ollama, &writer, &mut queue, &app).await;
            }
            _ = status_tick.tick() => {
                // Re-read config from disk so that provider/model changes saved in
                // Settings are reflected immediately without restarting the app.
                let cur_cfg = commands::load_config_sync();
                let vc = vision_count.lock().map(|c| *c).unwrap_or(0);
                let ec = event_count.lock().map(|c| *c).unwrap_or(0);
                if cur_cfg.provider == "openrouter" {
                    let or = OpenRouterClient::new(cur_cfg.openrouter_api_key.clone(), cur_cfg.openrouter_model.clone());
                    let ok = or.check_status().await;
                    let model = cur_cfg.openrouter_model.clone();
                    push_stats(&app, |s| {
                        s.provider = "openrouter".into();
                        s.active_model = model;
                        s.ollama_running = ok;
                        s.model_available = ok;
                        s.events_in_session = ec;
                        s.vision_snapshots = vc;
                    });
                } else {
                    let (running, has_model) = ollama.check_status_for_model(&cur_cfg.ollama_model).await;
                    let model = cur_cfg.ollama_model.clone();
                    push_stats(&app, |s| {
                        s.provider = "ollama".into();
                        s.active_model = model;
                        s.ollama_running = running;
                        s.model_available = has_model;
                        s.events_in_session = ec;
                        s.vision_snapshots = vc;
                    });
                }
            }
            _ = drain_tick.tick() => {
                if !queue.is_empty() {
                    tracing::info!("Drain tick: retrying {} queued session(s)", queue.len());
                    drain_queue(&ollama, &writer, &mut queue, &app).await;
                }
            }
        }
    }
}

/// Update the Tauri-managed SharedStats in place.
fn push_stats(app: &tauri::AppHandle, f: impl FnOnce(&mut state::PipelineStats)) {
    if let Some(shared) = app.try_state::<SharedStats>() {
        if let Ok(mut guard) = shared.0.lock() {
            f(&mut guard);
        }
    }
}

/// Drain queued sessions. Always writes a note — uses the configured LLM provider
/// if available, falls back to raw markdown otherwise.
async fn drain_queue(
    ollama: &OllamaClient,
    writer: &MarkdownWriter,
    queue: &mut Vec<Session>,
    app: &tauri::AppHandle,
) -> bool {
    let cfg = commands::load_config_sync();

    // Resolve provider readiness
    let (provider_ready, model_ready) = if cfg.provider == "openrouter" {
        let or = OpenRouterClient::new(cfg.openrouter_api_key.clone(), cfg.openrouter_model.clone());
        let ok = or.check_status().await;
        (ok, ok)
    } else {
        ollama.check_status_for_model(&cfg.ollama_model).await
    };

    if !provider_ready {
        tracing::warn!("{} offline — writing {} raw note(s)",
            if cfg.provider == "openrouter" { "OpenRouter" } else { "Ollama" },
            queue.len());
    } else if !model_ready {
        tracing::warn!("Model not available — writing {} raw note(s)", queue.len());
    }

    let sessions = std::mem::take(queue);
    for session in sessions {
        let result = if provider_ready && model_ready {
            let synthesis = if cfg.provider == "openrouter" {
                let or = OpenRouterClient::new(cfg.openrouter_api_key.clone(), cfg.openrouter_model.clone());
                or.synthesize_with_model(&session, &cfg.openrouter_model).await
            } else {
                ollama.synthesize_with_model(&session, &cfg.ollama_model).await
            };

            match synthesis {
                Ok(markdown) if !markdown.trim().is_empty() => writer.write(&session, &markdown),
                Ok(_) => {
                    tracing::warn!("Provider returned empty response — falling back to raw");
                    writer.write_raw(&session)
                }
                Err(e) => {
                    tracing::error!("Synthesis failed: {} — falling back to raw", e);
                    writer.write_raw(&session)
                }
            }
        } else {
            writer.write_raw(&session)
        };

        match result {
            Ok(note) => {
                tracing::info!("Note written: {}", note.file_path);
                push_stats(app, |s| {
                    s.total_notes += 1;
                    s.last_note_path = Some(note.file_path.clone());
                    s.ollama_running = provider_ready;
                    s.model_available = model_ready;
                });
            }
            Err(e) => tracing::error!("Failed to write note: {}", e),
        }
    }

    provider_ready && model_ready
}
