# LogR

A passive knowledge watcher for your desktop. LogR runs silently in the background, observes what you do on your computer, and writes AI-synthesized markdown notes — automatically, without interrupting your flow.

---

## What it does

LogR watches your activity in real time:

- **Which apps and documents** you focus on, and for how long
- **Browser tabs** you visit (title changes, URL navigation, SPA routing)
- **Files** you open and edit (with content snippets)
- **Clipboard** contents you copy
- **Typing bursts** — knows you were actively writing without logging what you typed
- **Vision snapshots** — periodically captures your screen and asks a local vision model to describe what you're doing

At natural break points (idle, category shift, or session cap), it groups the activity into a session and sends it to a local Ollama model, which writes a structured markdown note. If Ollama is unavailable, it writes a raw event log instead.

Notes land in `~/Documents/LogR/YYYY-MM-DD/` and are never sent anywhere.

---

## Privacy

LogR is fully local. Nothing leaves your machine.

- **No keylogging** — typing bursts count keystrokes only; key identities are never stored
- **No cloud** — Ollama runs locally; notes are written to your disk
- **Screenshots are never saved** — vision captures are encoded in memory, sent to Ollama, then discarded
- **Blocklist** — password managers, banking pages, login forms, and incognito windows are filtered out automatically
- **Communication apps** — Slack, Discord, Mail etc. are excluded by default (configurable)
- **Your notes directory** is excluded from filesystem watching so LogR never captures its own output

---

## How notes get written

Activity is grouped into **sessions**. A session flushes (and a note is written) when:

| Trigger | Default |
|---|---|
| You go idle | 2 minutes of no activity |
| App category changes | Coding → Browser → Terminal etc. |
| Session event cap | 30 events |
| Manual flush | Dashboard or tray menu |

---

## Requirements

- **Windows** (primary target; macOS support partial)
- **[Ollama](https://ollama.com)** running locally — used for both text synthesis and optional vision
- A pulled text model, e.g. `ollama pull gemma3:4b`
- *(Optional)* A vision model for screenshot descriptions, e.g. `ollama pull qwen3-vl:4b`

---

## Setup

1. **Install Ollama** and pull a model:
   ```
   ollama pull gemma3:4b
   ```

2. **Build and run LogR:**
   ```
   cd logr
   npm install
   npm run tauri dev
   ```
   Or build a release binary:
   ```
   npm run tauri build
   ```

3. **Open the dashboard** (LogR lives in your system tray).

4. **Configure in Settings:**
   - Set your Ollama model
   - Optionally set a vision model (e.g. `qwen3-vl:4b`) for screenshot descriptions
   - Adjust the idle timeout and minimum dwell time
   - Add apps or directories to block

---

## Events captured

| Event | What is recorded | Privacy filter |
|---|---|---|
| Window focus | App name, window title, dwell time | Blocked apps + title keywords |
| Browser tab change | Tab title, URL | Blocked URL keywords |
| Browser navigation | URL changes within same window | Blocked URL keywords |
| File edit | File path, first 600 chars of content | Hidden files excluded |
| File open | File path only | Hidden files excluded |
| Clipboard copy | Copied text | Passwords, emails, tokens dropped |
| Typing burst | Keystroke count + duration only | Blocked apps excluded |
| Vision snapshot | Ollama description of screen | Never stored; description only |

---

## Vision (screenshot descriptions)

When a vision model is configured, LogR:

1. Captures the primary monitor in memory (never written to disk)
2. Downscales to 1280px wide JPEG (~80–100 KB)
3. Sends to Ollama `/api/generate` with a brief description prompt
4. Attaches the description to the window event

Vision runs:
- When you first focus a window
- Every 45 seconds while you stay in the same window
- On every title/tab change

Recommended vision models: `qwen3-vl:4b` (fast, 3.3 GB), `qwen2-vl:7b`, `llava:7b`, `moondream:1.8b` (tiny, fastest).

---

## Notes format

Notes are written as markdown with YAML frontmatter:

```markdown
---
date: 2025-04-12T14:32:00Z
session_id: abc123
app: Cursor
topics: [rust, tauri, session-buffer]
duration: 18 min
events: 24
---

**Refactoring session buffer flush logic in LogR**

- Rewrote `force_flush()` in `session/mod.rs` to handle empty sessions gracefully
- Added `TypingBurst` and `BrowserNavigation` event types with prefilter rules
- Tested vision pipeline with `qwen3-vl:4b` — screenshot captured at 88 KB
- Browsed Tauri v2 docs for managed state patterns
```

If Ollama is unreachable at write time, a raw event log is written instead with a note at the top.

---

## Dashboard

The tray icon opens a small dashboard showing:

- Ollama connection + model status
- Events buffered in current session
- Notes written total
- Vision snapshots taken
- Last note written (clickable path)
- Flush Session button
- Test Note button

---

## Tech stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri v2 (Rust + WebView) |
| Frontend | React + TypeScript + Tailwind |
| Async runtime | Tokio |
| Window tracking | active-win-pos-rs |
| Keyboard hook | rdev |
| Screen capture | xcap + image |
| Clipboard | arboard |
| File watching | notify v6 |
| Process info | sysinfo |
| Browser URL | Windows UIAutomation |
| AI inference | Ollama (local) |
| Note storage | Markdown files on disk |

---

## Configuration

Config is stored at `~/.logr/config.json`:

```json
{
  "ollama_model": "gemma3:4b",
  "ollama_url": "http://localhost:11434",
  "vision_model": "qwen3-vl:4b",
  "notes_dir": "C:/Users/you/Documents/LogR",
  "session_idle_timeout_secs": 120,
  "min_dwell_secs": 10,
  "blocked_apps": [],
  "watch_dirs": [],
  "watch_communication_apps": false
}
```

All settings are editable in the Settings window without touching the file directly.

---

## Project structure

```
logr/
├── src/                        # React frontend
│   ├── App.tsx                 # Dashboard
│   └── Settings.tsx            # Settings window
└── src-tauri/
    └── src/
        ├── collectors/
        │   ├── window.rs       # Window focus + title change + vision
        │   ├── keyboard.rs     # Typing burst detection (no key content)
        │   ├── clipboard.rs    # Clipboard monitoring
        │   ├── filesystem.rs   # File open/edit watching
        │   ├── browser.rs      # URL change polling
        │   ├── screenshot.rs   # Screen capture + Ollama vision
        │   └── context.rs      # Browser URL + terminal context (UIAutomation)
        ├── session/            # Event buffering, flush logic, topic extraction
        ├── prefilter/          # Privacy filters (blocklists, credential detection)
        ├── synthesis/          # Ollama prompt building + note synthesis
        ├── writer/             # Markdown note writer
        ├── state/              # Shared app state, config, stats
        ├── commands.rs         # Tauri IPC commands
        ├── tray/               # System tray menu
        └── lib.rs              # Pipeline wiring
```

---

## License

MIT
