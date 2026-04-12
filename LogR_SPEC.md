# Driftlog — Passive Knowledge Watcher
## Full Build Specification for Claude Code

---

## 0. Overview

**Driftlog** is a lightweight, privacy-first, always-on desktop daemon that passively observes system activity and automatically generates structured markdown notes using a local LLM (Ollama). It requires zero manual input. It runs silently in the system tray, builds a searchable personal knowledge base from daily computer usage, and ships as a single installer.

### Design Principles
- **Local only** — no data ever leaves the machine. Non-negotiable.
- **Minimal footprint** — target <30MB RAM idle, <0.5% CPU when not synthesizing
- **Zero friction** — install and forget. Notes appear automatically.
- **Transparent** — user can always see exactly what was just logged
- **Open source** — clean repo, single binary output, MIT licensed

---

## 1. Tech Stack

| Layer | Choice | Reason |
|---|---|---|
| App shell | Tauri v2 | System tray, cross-platform installer, mobile path |
| Backend | Rust | Low memory, single binary, great concurrency |
| Frontend | React + TypeScript | Tray UI, settings panel |
| Styling | Tailwind CSS v4 | Utility-first, minimal bundle |
| LLM | Ollama (local) | Privacy-first, no API key needed |
| LLM Model | gemma3:4b (default) | Balance of quality and speed |
| Note format | Markdown (.md) | Universal, works with Obsidian/any viewer |

---

## 2. Project Structure

```
driftlog/
├── src-tauri/
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs                  # Tauri app entry, command registration
│       ├── lib.rs                   # Module declarations
│       ├── collectors/
│       │   ├── mod.rs               # Collector trait + shared types
│       │   ├── window.rs            # Active window collector
│       │   ├── clipboard.rs         # Clipboard change collector
│       │   └── filesystem.rs        # File open/edit event collector
│       ├── prefilter/
│       │   ├── mod.rs               # Filter trait
│       │   └── rules.rs             # All filter rules
│       ├── session/
│       │   ├── mod.rs               # Session buffer logic
│       │   └── types.rs             # Session, Event structs
│       ├── synthesis/
│       │   ├── mod.rs               # Orchestrator
│       │   └── ollama.rs            # Ollama HTTP client
│       ├── writer/
│       │   └── mod.rs               # Markdown file writer
│       ├── tray/
│       │   └── mod.rs               # System tray setup
│       └── state/
│           └── mod.rs               # App state (Arc<Mutex<>>)
├── src/
│   ├── main.tsx                     # React entry
│   ├── App.tsx                      # Root component + router
│   ├── components/
│   │   ├── TrayWindow.tsx           # Main tray popup window
│   │   ├── LiveFeed.tsx             # Real-time log transparency feed
│   │   ├── Settings.tsx             # Settings panel
│   │   └── NotePreview.tsx          # Last generated note preview
│   └── hooks/
│       └── useDriftlog.ts           # Tauri event listeners
├── package.json
└── SPEC.md                          # This file
```

---

## 3. Core Data Types (Rust)

```rust
// session/types.rs

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventType {
    WindowFocus,
    ClipboardChange,
    FileAccess,
    FileEdit,
    Idle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub timestamp: SystemTime,
    pub event_type: EventType,
    pub app: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,    // clipboard text, file path, etc.
    pub metadata: Option<String>,   // any extra context
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,                 // uuid
    pub started_at: SystemTime,
    pub ended_at: Option<SystemTime>,
    pub events: Vec<RawEvent>,
    pub dominant_app: Option<String>,
    pub topics: Vec<String>,        // extracted topic strings
    pub status: SessionStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionStatus {
    Active,
    Complete,
    Skipped,                        // pre-filter killed it
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedNote {
    pub session_id: String,
    pub content: String,            // raw markdown string
    pub file_path: String,
    pub created_at: SystemTime,
}

// Sent to frontend via Tauri events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedEntry {
    pub timestamp: String,
    pub event_type: String,
    pub summary: String,
    pub filtered: bool,             // was this dropped by pre-filter?
}
```

---

## 4. Collector Specifications

### 4.1 Collector Trait

```rust
// collectors/mod.rs

#[async_trait]
pub trait Collector: Send + Sync {
    fn name(&self) -> &str;
    async fn start(&self, tx: mpsc::Sender<RawEvent>);
}
```

Every collector runs as a spawned Tokio task, pushes `RawEvent` to the shared channel, and never blocks the main loop.

---

### 4.2 Window Collector (`collectors/window.rs`)

**Purpose:** Detect which app and document the user is focused on.

**Behavior:**
- Poll active window every **5 seconds**
- Only emit an event if window title **changed** since last poll
- Only emit if user has been on same window for **minimum 30 seconds** (dwell check)
- Capture: app name, window title

**Crate:** `active-win-pos-rs = "0.8"`

```rust
pub struct WindowCollector {
    pub poll_interval_secs: u64,    // default: 5
    pub min_dwell_secs: u64,        // default: 30
}
```

**Platform notes:**
- macOS: requires Accessibility permission — request on first launch
- Windows: works without special permissions
- Linux: requires X11 or Wayland — use `wmctrl` fallback if crate fails

---

### 4.3 Clipboard Collector (`collectors/clipboard.rs`)

**Purpose:** Capture text the user explicitly copies — strong intent signal.

**Behavior:**
- Poll clipboard every **3 seconds**
- Only emit if clipboard content **changed**
- Only emit if content is **plain text** (ignore images/files)
- Truncate content to **500 characters** before storing
- Skip if content matches any ignore pattern (see Pre-filter)

**Crate:** `arboard = "3.0"` (cross-platform, well-maintained)

```rust
pub struct ClipboardCollector {
    pub poll_interval_secs: u64,    // default: 3
    pub max_content_length: usize,  // default: 500
}
```

---

### 4.4 Filesystem Collector (`collectors/filesystem.rs`)

**Purpose:** Know which files the user is actively working with.

**Behavior:**
- Watch these directories: `~/Documents`, `~/Desktop`, `~/Projects`, `~/Developer`, `~/src`
- Emit event on: file **Create**, file **Modify** (debounced — max 1 event per file per 60s)
- Capture: file path, file extension, parent directory name
- **Ignore:** hidden files (`.git`, `.DS_Store`), node_modules, build outputs (`/dist`, `/target`, `/build`), binary files (non-text extensions)

**Crate:** `notify = "6.0"`

**Watched extensions (whitelist):**
```
.rs .go .py .js .ts .tsx .jsx .md .txt .json .yaml .yml
.toml .env .html .css .scss .sql .sh .swift .kt .dart
```

```rust
pub struct FilesystemCollector {
    pub watch_dirs: Vec<PathBuf>,
    pub debounce_secs: u64,         // default: 60
    pub allowed_extensions: Vec<String>,
}
```

---

## 5. Pre-filter Specification (`prefilter/rules.rs`)

The pre-filter runs **synchronously** on every raw event before it touches the session buffer. It is the most important component for note quality.

### 5.1 App Blocklist

Events from these apps are **silently dropped:**

```rust
pub const BLOCKED_APPS: &[&str] = &[
    "Finder", "Explorer", "Dock",
    "Spotify", "Apple Music", "VLC",
    "Netflix", "YouTube",           // also caught by title filter
    "System Preferences", "Settings",
    "Discord",                      // optional — user can whitelist
    "Slack",                        // optional — user can whitelist
    "Mail", "Outlook",              // privacy — don't log email content
    "1Password", "Keychain",
    "Calculator", "Clock",
    "Photos", "Preview",
];
```

### 5.2 Title Blocklist (substring match, case-insensitive)

```rust
pub const BLOCKED_TITLE_SUBSTRINGS: &[&str] = &[
    "password", "login", "sign in", "signin",
    "credit card", "billing", "payment",
    "youtube.com", "netflix.com", "twitch.tv",
    "pornhub", "incognito",
    "new tab", "blank page",
];
```

### 5.3 Clipboard Blocklist

Drop clipboard events if content:
- Is shorter than **10 characters**
- Matches email/password pattern (regex: contains `@` and `.` together, or > 20 char no spaces)
- Starts with `http` and is a bare URL with no other text
- Matches credit card pattern (16 digit number)

### 5.4 Dwell Filter

Drop `WindowFocus` events where `dwell_time < 30 seconds`. The user just glanced, not worked.

### 5.5 Filter Result

```rust
pub enum FilterResult {
    Allow,
    Drop(String),   // reason string, shown in UI transparency feed
}

pub fn apply(event: &RawEvent) -> FilterResult
```

---

## 6. Session Buffer (`session/mod.rs`)

### 6.1 What is a Session

A session is a **contiguous block of meaningful activity** around a coherent topic. Think of it as one "work chunk."

### 6.2 Session Boundaries

A session **ends** when any of the following occur:
- No meaningful event for **5 minutes** (idle timeout)
- Dominant app changes to a **different category** (e.g. coding → browser → messaging = 3 sessions)
- Session has accumulated **30+ events** (force flush, prevent unbounded growth)
- User manually triggers flush via tray UI

### 6.3 App Categories (for boundary detection)

```rust
pub enum AppCategory {
    Coding,         // VS Code, Cursor, Zed, Xcode, IntelliJ, etc.
    Browser,        // Chrome, Firefox, Safari, Arc, Brave
    Terminal,       // Terminal, iTerm, Warp, Kitty
    Writing,        // Notion, Obsidian, Word, Pages, Bear
    Communication,  // Slack, Discord, Teams, Mail — watch but don't log content
    Design,         // Figma, Sketch, Photoshop
    Other,
}
```

Only create session boundary when category **changes**. Switching between two coding apps (VS Code → Cursor) does not end the session.

### 6.4 Topic Extraction

Before sending to LLM, extract topic strings from the session events:

```rust
pub fn extract_topics(session: &Session) -> Vec<String> {
    // From window titles: extract filename, project name, domain
    // From file paths: extract filename without extension, parent dir
    // From clipboard: first 50 chars of each clipboard event
    // Deduplicate, max 10 topics
}
```

---

## 7. Ollama Integration (`synthesis/ollama.rs`)

### 7.1 Endpoint

```
POST http://localhost:11434/api/chat
```

Check Ollama is running before every call. If not running, queue the session and retry every 60 seconds.

### 7.2 Model Config

```rust
pub struct OllamaConfig {
    pub model: String,              // default: "gemma3:4b"
    pub base_url: String,           // default: "http://localhost:11434"
    pub temperature: f32,           // default: 0.3 (factual, not creative)
    pub max_tokens: u32,            // default: 512
}
```

### 7.3 Prompt Template

```rust
pub fn build_prompt(session: &Session) -> String {
    format!(r#"
You are a personal knowledge logger. Your only job is to write a concise markdown note 
summarising what the user just did on their computer.

Rules:
- Be factual and specific. Use the actual app names, file names, topics you can see.
- Maximum 6 bullet points.
- First line must be a single bold heading summarising the activity (e.g. **Debugging auth flow in VS Code**)
- Do not invent or assume anything not present in the data.
- Do not mention that you are an AI or that you are summarising.
- Output only markdown. No preamble, no explanation.

Activity data:
- Time: {} to {}
- Primary app: {}
- App category: {}
- Topics seen: {}
- Event count: {}

Write the markdown note now.
"#,
        format_time(session.started_at),
        format_time(session.ended_at.unwrap_or(SystemTime::now())),
        session.dominant_app.as_deref().unwrap_or("Unknown"),
        session_category_string(session),
        session.topics.join(", "),
        session.events.len()
    )
}
```

### 7.4 Request Shape

```rust
#[derive(Serialize)]
struct OllamaRequest {
    model: String,
    messages: Vec<OllamaMessage>,
    stream: bool,               // always false — we want complete response
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    temperature: f32,
    num_predict: u32,
}
```

### 7.5 Error Handling

| Error | Behaviour |
|---|---|
| Ollama not running | Queue session, show tray warning icon, retry every 60s |
| Model not found | Show notification: "Run: ollama pull gemma3:4b" |
| Timeout (>30s) | Discard session, log error to `~/.driftlog/errors.log` |
| Bad response | Discard, do not write partial note |

---

## 8. Markdown Writer (`writer/mod.rs`)

### 8.1 Output Directory

Default: `~/Documents/Driftlog/`
Configurable via settings.

### 8.2 Directory Structure

```
~/Documents/Driftlog/
├── 2026-04-10/
│   ├── 09-15_coding_vscode.md
│   ├── 11-30_browser_research.md
│   └── 14-20_writing_obsidian.md
├── 2026-04-11/
│   └── ...
└── index.md                    ← rolling daily summary, auto-updated
```

### 8.3 Filename Format

```
{HH-MM}_{category}_{dominant_app_slug}.md
```

Examples:
- `14-20_coding_vscode.md`
- `09-15_browser_arc.md`
- `16-45_writing_obsidian.md`

### 8.4 Note File Format

```markdown
---
date: 2026-04-10
time: 14:20 – 14:35
app: VS Code
category: Coding
topics: [renderer.ts, python-pptx, OpenGamma]
duration_mins: 15
event_count: 12
---

**Debugging font rendering in OpenGamma PPTX export**

- Reviewed `renderer.ts` focusing on dark theme slide generation
- Browsed 3 GitHub issues related to python-pptx font fallbacks
- Copied font-size fallback snippet from Stack Overflow
- Ran npm build twice, second run succeeded
- Working on explicit fallback in theme XML to fix dark mode font sizing
```

### 8.5 Index File

`index.md` is regenerated every time a new note is written. Format:

```markdown
# Driftlog — 2026-04-10

| Time | Activity | Duration |
|------|----------|----------|
| 09:15 | Coding in VS Code — OpenGamma renderer | 22 mins |
| 11:30 | Browser research — python-pptx issues | 15 mins |
| 14:20 | Writing in Obsidian — weekly review | 31 mins |
```

---

## 9. App State (`state/mod.rs`)

Single shared state wrapped in `Arc<Mutex<AppState>>`, passed to all Tauri commands.

```rust
pub struct AppState {
    pub is_watching: bool,
    pub session_buffer: Session,
    pub config: DriftlogConfig,
    pub recent_feed: VecDeque<FeedEntry>,   // last 50 entries for UI
    pub last_note: Option<GeneratedNote>,
    pub ollama_available: bool,
    pub notes_dir: PathBuf,
}

pub struct DriftlogConfig {
    pub ollama_model: String,
    pub ollama_url: String,
    pub notes_dir: String,
    pub session_idle_timeout_secs: u64,
    pub min_dwell_secs: u64,
    pub blocked_apps: Vec<String>,         // user additions to blocklist
    pub watch_dirs: Vec<String>,
    pub watch_communication_apps: bool,    // Slack/Discord — default false
}
```

Config persists to `~/.driftlog/config.json` via `serde_json`.

---

## 10. Tauri Commands

All commands are registered in `main.rs`. These are callable from the React frontend.

```rust
#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<AppStateSnapshot, String>

#[tauri::command]
async fn toggle_watching(state: State<'_, AppState>) -> Result<bool, String>

#[tauri::command]
async fn get_recent_feed(state: State<'_, AppState>) -> Result<Vec<FeedEntry>, String>

#[tauri::command]
async fn get_last_note(state: State<'_, AppState>) -> Result<Option<GeneratedNote>, String>

#[tauri::command]
async fn open_notes_folder(state: State<'_, AppState>) -> Result<(), String>

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<DriftlogConfig, String>

#[tauri::command]
async fn update_config(config: DriftlogConfig, state: State<'_, AppState>) -> Result<(), String>

#[tauri::command]
async fn force_flush_session(state: State<'_, AppState>) -> Result<(), String>

#[tauri::command]
async fn check_ollama_status() -> Result<OllamaStatus, String>
```

---

## 11. Tauri Events (Backend → Frontend)

Emitted via `app_handle.emit()`. Frontend listens with `listen()`.

```
driftlog://feed-entry      payload: FeedEntry         // new event captured or dropped
driftlog://note-generated  payload: GeneratedNote     // new note written to disk
driftlog://session-started payload: SessionSummary    // new session opened
driftlog://session-ended   payload: SessionSummary    // session flushed to LLM
driftlog://ollama-status   payload: OllamaStatus      // ollama up/down changes
driftlog://error           payload: ErrorEntry        // any background error
```

---

## 12. System Tray (`tray/mod.rs`)

### 12.1 Tray Icon States

| State | Icon | Tooltip |
|---|---|---|
| Watching, Ollama OK | Green dot | "Driftlog — Watching" |
| Watching, Ollama missing | Yellow dot | "Driftlog — Ollama not running" |
| Paused | Grey dot | "Driftlog — Paused" |

### 12.2 Tray Right-click Menu

```
● Driftlog
──────────────
▶ Open Dashboard
──────────────
⏸ Pause Watching    (toggle)
⚡ Flush Session Now
📁 Open Notes Folder
──────────────
⚙ Settings
──────────────
✕ Quit
```

### 12.3 Tray Click Behaviour

Left-click (or single click on Windows) opens the **Dashboard window** as a small floating panel (400px × 500px), positioned near the tray icon.

---

## 13. React UI Specification

### 13.1 Dashboard Window (`TrayWindow.tsx`)

A compact panel showing:

1. **Status bar** — watching/paused toggle pill, Ollama status dot
2. **Live Feed** (`LiveFeed.tsx`) — scrolling list of last 20 feed entries
   - Green row = allowed and logged
   - Grey row = filtered/dropped (with reason on hover)
   - Each row: `[time] [app icon] [title summary]`
3. **Last Note** (`NotePreview.tsx`) — last generated note in rendered markdown
4. **Footer** — "Open Notes Folder" button, settings gear icon

### 13.2 Settings Panel (`Settings.tsx`)

Sections:
- **LLM** — Ollama URL, model name, "Test Connection" button
- **Watching** — toggle per collector (window/clipboard/filesystem), dwell time slider
- **Notes** — notes directory path, "Open in Finder" button
- **Privacy** — custom blocked apps input, view current blocklist
- **Danger zone** — "Clear all notes", "Reset config"

### 13.3 Design Direction

- **Dark theme only** — suits a background system tool
- Font: `JetBrains Mono` for feed entries (feels like a terminal log), `Inter` for settings
- Accent color: `#22D3EE` (cyan) — active/watching state
- Muted: `#6B7280` (grey) — filtered/inactive state
- Background: `#0F0F0F`, surface: `#1A1A1A`, border: `#2A2A2A`
- No decorative elements — purely functional, information-dense

---

## 14. Cargo.toml Dependencies

```toml
[package]
name = "driftlog"
version = "0.1.0"
edition = "2021"

[dependencies]
tauri = { version = "2", features = ["tray-icon", "image-png"] }
tauri-plugin-shell = "2"
tauri-plugin-notification = "2"
tauri-plugin-fs = "2"

# Async runtime
tokio = { version = "1", features = ["full"] }

# System monitoring
active-win-pos-rs = "0.8"
arboard = "3"
notify = "6"
sysinfo = "0.30"

# HTTP (for Ollama)
reqwest = { version = "0.12", features = ["json"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Utilities
uuid = { version = "1", features = ["v4"] }
chrono = { version = "0.4", features = ["serde"] }
dirs = "5"
regex = "1"
tracing = "0.1"
tracing-subscriber = "0.3"

[features]
default = ["custom-protocol"]
custom-protocol = ["tauri/custom-protocol"]
```

---

## 15. tauri.conf.json Key Settings

```json
{
  "productName": "Driftlog",
  "version": "0.1.0",
  "identifier": "io.driftlog.app",
  "build": {
    "frontendDist": "../dist",
    "devUrl": "http://localhost:1420"
  },
  "app": {
    "windows": [
      {
        "label": "dashboard",
        "title": "Driftlog",
        "width": 400,
        "height": 500,
        "resizable": false,
        "decorations": false,
        "alwaysOnTop": true,
        "skipTaskbar": true,
        "visible": false
      }
    ],
    "trayIcon": {
      "iconPath": "icons/tray.png",
      "iconAsTemplate": true
    }
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "icon": ["icons/32x32.png", "icons/128x128.png", "icons/icon.icns", "icons/icon.ico"]
  }
}
```

---

## 16. Permissions (macOS `Info.plist` additions)

```xml
<key>NSAppleEventsUsageDescription</key>
<string>Driftlog needs this to detect the active application.</string>

<key>NSSystemAdministrationUsageDescription</key>
<string>Driftlog uses this to monitor active window titles.</string>
```

macOS Accessibility permission must be requested at runtime on first launch via a native dialog.

---

## 17. Build & Install

### Development
```bash
npm install
npm run tauri dev
```

### Production build
```bash
npm run tauri build
```

Outputs:
- macOS: `target/release/bundle/dmg/Driftlog_0.1.0_aarch64.dmg`
- Windows: `target/release/bundle/msi/Driftlog_0.1.0_x64.msi`
- Linux: `target/release/bundle/appimage/driftlog_0.1.0_amd64.AppImage`

### Ollama setup (user requirement)
```bash
# User must have Ollama installed
brew install ollama          # macOS
ollama pull gemma3:4b        # pull default model
ollama serve                 # start server (Driftlog checks this on launch)
```

Driftlog should detect if Ollama is missing and show a first-run setup screen with these exact commands.

---

## 18. Implementation Order for Claude Code

Follow this exact sequence. Do not skip ahead. Each phase produces working, runnable output.

### Phase 1 — Scaffold
1. Create Tauri v2 project with React + TypeScript + Tailwind
2. Set up all Rust modules as empty stubs (mod.rs files with todo!() bodies)
3. Verify `npm run tauri dev` compiles and opens a blank window
4. Add system tray icon with basic right-click menu (hardcoded, no logic yet)

### Phase 2 — Collectors
1. Implement `WindowCollector` with dwell logic — print events to console only
2. Implement `ClipboardCollector` — print changes to console
3. Implement `FilesystemCollector` — print file events to console
4. Wire all three into a shared `mpsc::channel` in `main.rs`
5. Verify all three collectors run concurrently and print events

### Phase 3 — Pre-filter + Session Buffer
1. Implement `prefilter/rules.rs` with all blocklists
2. Implement `SessionBuffer` with idle timeout and category-change boundary detection
3. Wire: collectors → pre-filter → session buffer
4. Add `FeedEntry` emission so filtered/allowed decisions are visible

### Phase 4 — Ollama Synthesis
1. Implement `OllamaClient` with connection check
2. Implement `build_prompt()` 
3. Wire: completed session → Ollama → markdown string
4. Test with a hardcoded dummy session first before wiring live sessions

### Phase 5 — Markdown Writer
1. Implement directory creation and filename generation
2. Implement note file writing with YAML frontmatter
3. Implement `index.md` regeneration
4. Wire: Ollama response → writer

### Phase 6 — App State + Tauri Commands
1. Implement `AppState` with `Arc<Mutex<>>`
2. Implement all Tauri commands
3. Emit all backend events via `app_handle.emit()`
4. Test commands via browser devtools console in dev mode

### Phase 7 — React UI
1. Build `TrayWindow.tsx` layout shell
2. Build `LiveFeed.tsx` with mock data
3. Build `NotePreview.tsx` with rendered markdown
4. Wire to real Tauri events and commands
5. Build `Settings.tsx` with all config fields

### Phase 8 — Polish + Build
1. Tray icon state changes (green/yellow/grey)
2. macOS Accessibility permission request on first launch
3. First-run Ollama setup screen
4. `npm run tauri build` — produce installer for target platform

---

## 19. Out of Scope for v1

These are intentional exclusions. Do not implement in v1.

- Browser extension for tab URL capture
- Terminal/shell command capture
- Screenshot capture
- Cloud sync
- Mobile app (Phase 2 of the product)
- Multi-device sync
- Any network calls outside of localhost Ollama

---

## 20. File: `~/.driftlog/`

Driftlog's own config/data directory (separate from notes output):

```
~/.driftlog/
├── config.json          ← user settings
├── errors.log           ← background errors
└── queue/               ← sessions waiting for Ollama (if it was offline)
    └── {session-id}.json
```

---

*End of spec. Begin with Phase 1.*
