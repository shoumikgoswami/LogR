use async_trait::async_trait;
use chrono::Utc;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

use super::{Collector, RawEvent};
use crate::session::types::EventType;

pub struct FilesystemCollector {
    pub watch_dirs: Vec<PathBuf>,
    /// Directories whose contents should never be emitted as events (e.g. LogR's own notes dir).
    pub exclude_dirs: Vec<PathBuf>,
    pub debounce_secs: u64,
    pub allowed_extensions: Vec<String>,
}

impl FilesystemCollector {
    pub fn new() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let mut watch_dirs = vec![home.join("Documents"), home.join("Desktop")];
        for extra in &["Projects", "src", "Developer"] {
            let p = home.join(extra);
            if p.exists() {
                watch_dirs.push(p);
            }
        }

        let allowed_extensions = vec![
            "rs", "go", "py", "js", "ts", "tsx", "jsx", "md", "txt", "json", "yaml", "yml",
            "toml", "html", "css", "scss", "sql", "sh", "swift", "kt", "dart", "env",
        ]
        .into_iter()
        .map(String::from)
        .collect();

        // Always exclude LogR's own notes directory so we don't capture our own output
        let notes_dir = dirs::document_dir()
            .unwrap_or_default()
            .join("LogR");

        Self {
            watch_dirs,
            exclude_dirs: vec![notes_dir],
            debounce_secs: 60,
            allowed_extensions,
        }
    }

    fn read_file_snippet(path: &Path, max_chars: usize) -> Option<String> {
        // Skip binary-ish files and very large files
        let meta = std::fs::metadata(path).ok()?;
        if meta.len() > 1_000_000 {
            return None; // skip files > 1 MB
        }
        let text = std::fs::read_to_string(path).ok()?;
        if text.is_empty() {
            return None;
        }
        // Take the first `max_chars` characters and clean up
        let snippet: String = text.chars().take(max_chars).collect();
        // Trim trailing incomplete line
        let snippet = if snippet.len() == max_chars {
            match snippet.rfind('\n') {
                Some(pos) => snippet[..pos].to_string(),
                None => snippet,
            }
        } else {
            snippet
        };
        Some(snippet.trim().to_string())
    }

    fn is_path_allowed(path: &Path, allowed_extensions: &[String], exclude_dirs: &[PathBuf]) -> bool {
        // Never watch hidden files/directories
        let has_hidden = path
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
        if has_hidden {
            return false;
        }

        // Never watch excluded directories (e.g. LogR's own notes output)
        for excl in exclude_dirs {
            if path.starts_with(excl) {
                return false;
            }
        }

        let path_str = path.to_string_lossy().to_lowercase();
        for blocked in &[
            "node_modules",
            "\\dist\\",
            "/dist/",
            "\\target\\",
            "/target/",
            "\\build\\",
            "/build/",
            "\\.git\\",
            "/.git/",
        ] {
            if path_str.contains(blocked) {
                return false;
            }
        }

        if let Some(ext) = path.extension() {
            let ext_s = ext.to_string_lossy().to_lowercase();
            return allowed_extensions.iter().any(|e| e == &ext_s);
        }

        false
    }
}

#[async_trait]
impl Collector for FilesystemCollector {
    fn name(&self) -> &str {
        "filesystem"
    }

    async fn start(&self, tx: mpsc::Sender<RawEvent>) {
        let debounce_duration = Duration::from_secs(self.debounce_secs);
        let allowed_extensions = self.allowed_extensions.clone();
        let watch_dirs = self.watch_dirs.clone();
        let exclude_dirs = self.exclude_dirs.clone();

        // Debounce map shared within this async task
        let debounce_map: Arc<Mutex<HashMap<PathBuf, Instant>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // Bridge: sync notify callback → async consumer via Arc<Mutex<Receiver>>
        let (std_tx, std_rx) =
            std::sync::mpsc::channel::<(PathBuf, EventType)>();
        let std_rx = Arc::new(Mutex::new(std_rx));

        // Spawn OS thread for the watcher
        let exts_clone = allowed_extensions.clone();
        let excl_clone = exclude_dirs.clone();
        std::thread::spawn(move || {
            let tx_clone = std_tx.clone();
            let mut watcher = match notify::recommended_watcher(
                move |res: notify::Result<Event>| {
                    let event = match res {
                        Ok(e) => e,
                        Err(_) => return,
                    };

                    let event_type = match event.kind {
                        EventKind::Create(_) => EventType::FileAccess,
                        EventKind::Modify(_) => EventType::FileEdit,
                        _ => return,
                    };

                    for path in &event.paths {
                        if FilesystemCollector::is_path_allowed(path, &exts_clone, &excl_clone) {
                            let _ = tx_clone.send((path.clone(), event_type.clone()));
                        }
                    }
                },
            ) {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("Failed to create filesystem watcher: {}", e);
                    return;
                }
            };

            for dir in &watch_dirs {
                if dir.exists() {
                    if let Err(e) = watcher.watch(dir, RecursiveMode::Recursive) {
                        tracing::warn!("Failed to watch {:?}: {}", dir, e);
                    } else {
                        tracing::info!("Watching directory: {:?}", dir);
                    }
                }
            }

            // Keep watcher alive indefinitely
            loop {
                std::thread::sleep(Duration::from_secs(3600));
            }
        });

        // Async polling loop: drain events from std channel with debounce
        loop {
            let rx = std_rx.clone();
            let received = tokio::task::spawn_blocking(move || {
                let guard = rx.lock().unwrap();
                guard.recv_timeout(Duration::from_millis(500)).ok()
            })
            .await;

            let (path, event_type) = match received {
                Ok(Some(v)) => v,
                _ => continue,
            };

            // Debounce check
            {
                let mut map = debounce_map.lock().unwrap();
                let now = Instant::now();
                if let Some(last) = map.get(&path) {
                    if now.duration_since(*last) < debounce_duration {
                        continue;
                    }
                }
                map.insert(path.clone(), now);
            }

            // For file edits, read a snippet of the file so notes capture actual content
            let file_snippet = if matches!(event_type, EventType::FileEdit) {
                FilesystemCollector::read_file_snippet(&path, 600)
            } else {
                None
            };

            let event = RawEvent {
                timestamp: Utc::now(),
                event_type,
                app: None,
                title: None,
                content: Some(path.to_string_lossy().into_owned()),
                context: file_snippet,
                metadata: path
                    .parent()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned()),
            };

            if tx.send(event).await.is_err() {
                break;
            }
        }
    }
}
