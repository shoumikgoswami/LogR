import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import Settings from "./Settings";

const win = getCurrentWindow();

interface PipelineStats {
  events_in_session: number;
  total_notes: number;
  vision_snapshots: number;
  ollama_running: boolean;
  model_available: boolean;
  is_watching: boolean;
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

  async function handleTestNote() {
    try {
      const path = await invoke<string>("write_test_note");
      setToast(`Test note written:\n${path.split(/[\\/]/).slice(-2).join("/")}`);
    } catch (e) {
      setToast(`Failed: ${e}`);
    }
  }

  const [toast, setToast] = useState<string | null>(null);
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 4000);
    return () => clearTimeout(t);
  }, [toast]);

  const ollamaLabel = !stats
    ? "—"
    : stats.ollama_running
      ? stats.model_available ? "Connected" : "Running — model not pulled"
      : "Offline";
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
        <div className="flex items-center gap-2 py-2" style={{ borderBottom: "1px solid var(--color-border)" }}>
          <Dot color={stats ? "#22c55e" : "var(--color-muted)"} />
          <span className="text-xs font-medium" style={{ color: "#e5e7eb" }}>
            {stats ? "Watching" : "Starting…"}
          </span>
        </div>
        <StatRow label="Ollama" value={ollamaLabel} accent={!!stats?.ollama_running} />
        <StatRow label="Events buffered" value={String(stats?.events_in_session ?? 0)} accent={!!stats && stats.events_in_session > 0} />
        <StatRow label="Notes written" value={String(stats?.total_notes ?? 0)} accent={!!stats && stats.total_notes > 0} />
        <StatRow label="Vision snapshots" value={String(stats?.vision_snapshots ?? 0)} accent={!!stats && (stats.vision_snapshots ?? 0) > 0} />
        <div className="flex items-center justify-between py-1.5">
          <span className="text-xs" style={{ color: "var(--color-muted)" }}>Last note</span>
          <span className="text-xs" style={{ color: "#e5e7eb", maxWidth: 180, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
            title={stats?.last_note_path ?? ""}>{lastNote}</span>
        </div>
      </div>

      {/* Ollama warning */}
      {stats && !stats.ollama_running && (
        <div className="mx-4 mt-3 px-3 py-2 rounded text-xs"
          style={{ background: "#2d1f00", border: "1px solid #78350f", color: "#fbbf24" }}>
          Ollama is offline. Start it with <code className="font-mono">ollama serve</code>.
          Sessions are queued and will be processed when it comes back.
        </div>
      )}
      {stats && stats.ollama_running && !stats.model_available && (
        <div className="mx-4 mt-3 px-3 py-2 rounded text-xs"
          style={{ background: "#1a1a00", border: "1px solid #713f12", color: "#fbbf24" }}>
          Model not pulled. Run <code className="font-mono">ollama pull gemma3:4b</code> to enable AI summaries.
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
        <button onClick={handleTestNote}
          className="w-full text-xs py-2 rounded"
          style={{ background: "var(--color-border)", color: "var(--color-muted)", border: "1px solid var(--color-border)", cursor: "pointer" }}>
          🧪 Write Test Note
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
