import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import Settings from "./Settings";

const win = getCurrentWindow();

interface PipelineStats {
  events_in_session: number;
  total_notes: number;
  vision_snapshots: number;
  provider: string;
  active_model: string;
  ollama_running: boolean;
  model_available: boolean;
  is_watching: boolean;
  is_paused: boolean;
  last_note_path: string | null;
}

function LogRLogo({ size = 48 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 48 48" fill="none">
      <path d="M4 24 C10 14, 20 10, 24 10 C28 10, 38 14, 44 24 C38 34, 28 38, 24 38 C20 38, 10 34, 4 24 Z"
        stroke="#22D3EE" strokeWidth="2" fill="none" strokeLinejoin="round" />
      <circle cx="24" cy="24" r="7" stroke="#22D3EE" strokeWidth="2" fill="none" />
      <circle cx="24" cy="24" r="2.5" fill="#22D3EE" />
      <polyline points="10,34 14,34 16,30 18,38 20,34 22,34 24,31 26,34 28,34"
        stroke="#22D3EE" strokeWidth="1.5" fill="none" strokeLinecap="round" strokeLinejoin="round" opacity="0.7" />
    </svg>
  );
}

function Dot({ color }: { color: string }) {
  return <span style={{ display: "inline-block", width: 7, height: 7, borderRadius: "50%", background: color, flexShrink: 0 }} />;
}

function StatRow({ label, value, accent }: { label: string; value: string; accent?: boolean }) {
  return (
    <div className="flex items-center justify-between py-1.5"
      style={{ borderBottom: "1px solid var(--color-border)" }}>
      <span className="text-xs" style={{ color: "var(--color-muted)" }}>{label}</span>
      <span className="text-xs font-medium" style={{ color: accent ? "var(--color-accent)" : "#e5e7eb" }}>{value}</span>
    </div>
  );
}

function Dashboard() {
  const [stats, setStats] = useState<PipelineStats | null>(null);
  const [screenRecordingOk, setScreenRecordingOk] = useState<boolean | null>(null);

  useEffect(() => {
    // Debounce the hide so a brief focus-away (e.g. during the tray-click →
    // show → focus sequence on Windows) doesn't instantly collapse the window.
    let hideTimer: ReturnType<typeof setTimeout> | null = null;

    const handleBlur = () => {
      hideTimer = setTimeout(() => win.hide(), 200);
    };
    const handleFocus = () => {
      if (hideTimer !== null) {
        clearTimeout(hideTimer);
        hideTimer = null;
      }
    };

    window.addEventListener("blur", handleBlur);
    window.addEventListener("focus", handleFocus);
    return () => {
      window.removeEventListener("blur", handleBlur);
      window.removeEventListener("focus", handleFocus);
      if (hideTimer !== null) clearTimeout(hideTimer);
    };
  }, []);

  // Poll status every 3 seconds while visible
  useEffect(() => {
    const fetch = () =>
      invoke<PipelineStats>("get_status")
        .then(setStats)
        .catch(() => {});

    fetch();
    const id = setInterval(fetch, 3000);
    return () => clearInterval(id);
  }, []);

  // One-time check for macOS Screen Recording permission
  useEffect(() => {
    invoke<boolean>("check_macos_permissions")
      .then(setScreenRecordingOk)
      .catch(() => setScreenRecordingOk(true)); // non-macOS always passes
  }, []);

  async function handleFlush() {
    if (stats && stats.events_in_session === 0) {
      setToast("No events to flush — keep using your computer first.");
      return;
    }
    try {
      await invoke("flush_session");
      setToast(`Flushing ${stats?.events_in_session ?? 0} events — note will appear shortly.`);
    } catch (e) {
      setToast(`Flush failed: ${e}`);
    }
  }

  async function handleTogglePause() {
    try {
      const nowPaused = await invoke<boolean>("toggle_pause");
      setStats((s) => s ? { ...s, is_paused: nowPaused, is_watching: !nowPaused } : s);
    } catch (e) {
      setToast("Failed to toggle pause: " + e);
    }
  }

  async function handleToggleProvider() {
    if (!stats) return;
    const next = stats.provider === "ollama" ? "openrouter" : "ollama";
    try {
      await invoke("set_provider", { provider: next });
      // Optimistically update local display immediately
      setStats((s) => s ? { ...s, provider: next, active_model: "checking…", ollama_running: false, model_available: false } : s);
      // Then do a real connectivity check in the background
      invoke("refresh_provider_status").then(() =>
        invoke<PipelineStats>("get_status").then(setStats).catch(() => {})
      ).catch(() => {});
    } catch (e) {
      setToast("Failed to switch provider: " + e);
    }
  }

  async function openSettings() {
    const sw = await WebviewWindow.getByLabel("settings");
    if (sw) {
      await sw.show();
      await sw.setFocus();
    }
  }

  async function handleTestNote() {
    try {
      const path = await invoke<string>("write_test_note");
      setToast(`Test note written:\n${path.split(/[\\/]/).slice(-2).join("/")}`);
    } catch (e) {
      setToast(`Failed: ${e}`);
    }
  }

  async function handleDailySummary() {
    setToast("Generating yesterday's summary…");
    try {
      const path = await invoke<string>("generate_daily_summary", { date: "" });
      setToast(`Summary written:\n${path.split(/[\\/]/).slice(-3).join("/")}`);
    } catch (e) {
      const msg = String(e);
      if (msg.includes("already exists") || msg.includes("daily_summary")) {
        setToast("Summary already exists for yesterday.");
      } else {
        setToast(`Summary failed: ${msg}`);
      }
    }
  }

  const [toast, setToast] = useState<string | null>(null);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  const providerLabel = !stats
    ? "—"
    : stats.provider === "openrouter" ? "OpenRouter" : "Ollama (local)";

  const providerStatus = !stats
    ? "—"
    : stats.ollama_running
      ? stats.model_available ? "Connected ✓" : "Reachable — model not ready"
      : "Offline";

  const modelLabel = stats?.active_model
    ? stats.active_model.length > 28
      ? "…" + stats.active_model.slice(-26)
      : stats.active_model
    : "—";

  const lastNote = stats?.last_note_path
    ? stats.last_note_path.split(/[\\/]/).slice(-2).join("/")
    : "None yet";

  return (
    <div className="flex flex-col h-screen select-none"
      style={{ background: "var(--color-bg)", border: "1px solid var(--color-border)" }}>

      {/* Title bar */}
      <div className="flex items-center justify-between px-3 py-2"
        style={{ borderBottom: "1px solid var(--color-border)" }}
        data-tauri-drag-region>
        <span className="text-xs font-semibold tracking-widest uppercase" style={{ color: "var(--color-muted)" }}>
          LogR
        </span>
        <button onClick={() => win.hide()}
          className="flex items-center justify-center w-5 h-5 rounded"
          style={{ color: "var(--color-muted)" }}
          onMouseEnter={(e) => (e.currentTarget.style.color = "#ef4444")}
          onMouseLeave={(e) => (e.currentTarget.style.color = "var(--color-muted)")}
          title="Close">
          <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
            <line x1="1" y1="1" x2="9" y2="9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            <line x1="9" y1="1" x2="1" y2="9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
        </button>
      </div>

      {/* Logo + name */}
      <div className="flex flex-col items-center gap-2 pt-6 pb-4">
        <LogRLogo size={48} />
        <div className="text-center">
          <h1 className="text-xl font-bold tracking-wide" style={{ color: "var(--color-accent)" }}>LogR</h1>
          <p className="text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>Passive Knowledge Watcher</p>
        </div>
      </div>

      {/* Status panel */}
      <div className="mx-4 rounded-lg px-3" style={{ background: "var(--color-surface)", border: "1px solid var(--color-border)" }}>
        <div className="flex items-center justify-between py-2" style={{ borderBottom: "1px solid var(--color-border)" }}>
          <div className="flex items-center gap-2">
            <Dot color={!stats ? "var(--color-muted)" : stats.is_paused ? "#f59e0b" : "#22c55e"} />
            <span className="text-xs font-medium" style={{ color: "#e5e7eb" }}>
              {!stats ? "Starting…" : stats.is_paused ? "Paused" : "Watching"}
            </span>
          </div>
          {stats && (
            <button
              onClick={handleTogglePause}
              className="text-xs px-2 py-0.5 rounded"
              style={{
                background: "var(--color-border)",
                color: stats.is_paused ? "#22c55e" : "#f59e0b",
                border: `1px solid ${stats.is_paused ? "#22c55e" : "#f59e0b"}`,
                cursor: "pointer",
              }}
              title={stats.is_paused ? "Resume watching" : "Pause watching"}>
              {stats.is_paused ? "▶ Resume" : "⏸ Pause"}
            </button>
          )}
        </div>
        {/* Clickable provider row — tap to switch */}
        <div className="flex items-center justify-between py-1.5"
          style={{ borderBottom: "1px solid var(--color-border)", cursor: stats ? "pointer" : "default" }}
          onClick={stats ? handleToggleProvider : undefined}
          title="Click to switch provider">
          <span className="text-xs" style={{ color: "var(--color-muted)" }}>Provider</span>
          <div className="flex items-center gap-1.5">
            <span className="text-xs font-medium" style={{ color: stats?.ollama_running ? "var(--color-accent)" : "#e5e7eb" }}>
              {providerLabel}
            </span>
            {stats && (
              <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="var(--color-muted)"
                strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
                <path d="M7 16V4m0 0L3 8m4-4l4 4" /><path d="M17 8v12m0 0l4-4m-4 4l-4-4" />
              </svg>
            )}
          </div>
        </div>
        <StatRow label="Model" value={modelLabel} accent={!!stats?.ollama_running} />
        <StatRow label="Status" value={providerStatus} accent={!!stats?.ollama_running} />
        <StatRow label="Events buffered" value={String(stats?.events_in_session ?? 0)} accent={!!stats && stats.events_in_session > 0} />
        <StatRow label="Notes written" value={String(stats?.total_notes ?? 0)} accent={!!stats && stats.total_notes > 0} />
        <StatRow label="Vision snapshots" value={String(stats?.vision_snapshots ?? 0)} accent={!!stats && (stats.vision_snapshots ?? 0) > 0} />
        <div className="flex items-center justify-between py-1.5">
          <span className="text-xs" style={{ color: "var(--color-muted)" }}>Last note</span>
          <span className="text-xs" style={{ color: "#e5e7eb", maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
            title={stats?.last_note_path ?? ""}>{lastNote}</span>
        </div>
      </div>

      {/* macOS Screen Recording permission warning */}
      {screenRecordingOk === false && (
        <div className="mx-4 mt-3 px-3 py-2 rounded text-xs"
          style={{ background: "#1e1a2e", border: "1px solid #7c3aed", color: "#c4b5fd" }}>
          ⚠ Screen Recording permission not granted. LogR can't track windows or take vision snapshots.
          {" "}<strong>System Settings → Privacy &amp; Security → Screen Recording → enable LogR</strong>, then relaunch.
        </div>
      )}

      {/* Provider warning */}
      {stats && !stats.ollama_running && stats.provider === "ollama" && (
        <div className="mx-4 mt-3 px-3 py-2 rounded text-xs"
          style={{ background: "#2d1f00", border: "1px solid #78350f", color: "#fbbf24" }}>
          Ollama is offline. Start it with <code className="font-mono">ollama serve</code>.
          Sessions are queued and will be processed when it comes back.
        </div>
      )}
      {stats && !stats.ollama_running && stats.provider === "openrouter" && (
        <div className="mx-4 mt-3 px-3 py-2 rounded text-xs"
          style={{ background: "#2d1f00", border: "1px solid #78350f", color: "#fbbf24" }}>
          OpenRouter unreachable — check your API key in Settings.
          Sessions are queued and will be processed when it comes back.
        </div>
      )}
      {stats && stats.ollama_running && !stats.model_available && stats.provider === "ollama" && (
        <div className="mx-4 mt-3 px-3 py-2 rounded text-xs"
          style={{ background: "#1a1a00", border: "1px solid #713f12", color: "#fbbf24" }}>
          Model not pulled. Run <code className="font-mono">ollama pull {stats.active_model || "gemma3:4b"}</code> to enable AI summaries.
          Notes are being written in raw format until then.
        </div>
      )}

      {/* Buttons */}
      <div className="flex-1" />
      <div className="px-4 pb-4 pt-2 flex flex-col gap-2">
        <button onClick={handleFlush}
          className="w-full text-xs py-2 rounded"
          style={{ background: "var(--color-border)", color: "var(--color-accent)", border: "1px solid var(--color-accent)", cursor: "pointer" }}>
          ⚡ Flush Session Now
        </button>
        <div className="flex gap-2">
          <button onClick={handleTestNote}
            className="flex-1 text-xs py-2 rounded"
            style={{ background: "var(--color-border)", color: "var(--color-muted)", border: "1px solid var(--color-border)", cursor: "pointer" }}>
            🧪 Test Note
          </button>
          <button onClick={openSettings}
            className="flex-1 text-xs py-2 rounded"
            style={{ background: "var(--color-border)", color: "var(--color-muted)", border: "1px solid var(--color-border)", cursor: "pointer" }}>
            ⚙ Settings
          </button>
        </div>
        <button onClick={handleDailySummary}
          className="w-full text-xs py-2 rounded"
          style={{ background: "var(--color-border)", color: "var(--color-muted)", border: "1px solid var(--color-border)", cursor: "pointer" }}>
          📋 Yesterday's Summary
        </button>
      </div>

      {/* Toast */}
      {toast && (
        <div className="absolute bottom-4 left-3 right-3 px-3 py-2 rounded text-xs"
          style={{ background: "var(--color-surface)", border: "1px solid var(--color-border)", color: "#e5e7eb", whiteSpace: "pre-line" }}>
          {toast}
        </div>
      )}
    </div>
  );
}

export default function App() {
  return win.label === "settings" ? <Settings /> : <Dashboard />;
}
