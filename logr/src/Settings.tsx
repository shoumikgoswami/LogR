import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { openPath } from "@tauri-apps/plugin-opener";

const win = getCurrentWindow();

// ── Shared primitives ──────────────────────────────────────────

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <div className="mb-5">
      <p className="text-xs font-semibold uppercase tracking-widest mb-2" style={{ color: "var(--color-muted)" }}>
        {title}
      </p>
      <div className="rounded-lg p-3 flex flex-col gap-3"
        style={{ background: "var(--color-surface)", border: "1px solid var(--color-border)" }}>
        {children}
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <label className="text-xs" style={{ color: "var(--color-muted)" }}>{label}</label>
      {children}
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  background: "#0F0F0F",
  border: "1px solid var(--color-border)",
  borderRadius: 6,
  color: "#e5e7eb",
  padding: "5px 8px",
  fontSize: 13,
  outline: "none",
  width: "100%",
};

function Toggle({ value, onChange }: { value: boolean; onChange: (v: boolean) => void }) {
  return (
    <div
      onClick={() => onChange(!value)}
      style={{
        width: 36, height: 20, borderRadius: 10, cursor: "pointer", position: "relative",
        background: value ? "var(--color-accent)" : "var(--color-border)",
        transition: "background 0.2s", flexShrink: 0,
      }}
    >
      <div style={{
        width: 14, height: 14, borderRadius: "50%", background: "#fff",
        position: "absolute", top: 3, left: value ? 19 : 3, transition: "left 0.2s",
      }} />
    </div>
  );
}

function ToggleRow({ label, value, onChange }: { label: string; value: boolean; onChange: (v: boolean) => void }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-sm" style={{ color: "#e5e7eb" }}>{label}</span>
      <Toggle value={value} onChange={onChange} />
    </div>
  );
}

// ── Vision model picker ────────────────────────────────────────

/** Vision-capable models available to pull — shown as suggestions if not installed */
const PULLABLE_VISION_MODELS = [
  { name: "qwen3-vl:4b",            label: "qwen3-vl:4b — Qwen3 vision, 4B (recommended, fast)" },
  { name: "qwen3-vl:7b",            label: "qwen3-vl:7b — Qwen3 vision, 7B" },
  { name: "qwen2-vl:7b",            label: "qwen2-vl:7b — Qwen2 vision, 7B" },
  { name: "qwen2-vl:72b",           label: "qwen2-vl:72b — Qwen2 vision, 72B" },
  { name: "gemma3:4b",              label: "gemma3:4b — multimodal, 4B" },
  { name: "gemma3:12b",             label: "gemma3:12b — multimodal, 12B" },
  { name: "llama3.2-vision:11b",    label: "llama3.2-vision:11b — Meta vision, 11B" },
  { name: "llava:7b",               label: "llava:7b — LLaVA, 7B" },
  { name: "llava-llama3:8b",        label: "llava-llama3:8b — LLaVA + Llama 3, 8B" },
  { name: "moondream:1.8b",         label: "moondream:1.8b — tiny vision model, 1.8B" },
  { name: "minicpm-v:8b",           label: "minicpm-v:8b — MiniCPM vision, 8B" },
  { name: "phi4-vision:14b",        label: "phi4-vision:14b — Microsoft Phi-4 vision, 14B" },
  { name: "mistral-small3.1:24b",   label: "mistral-small3.1:24b — Mistral vision, 24B" },
];

function VisionModelPicker({
  value, onChange, localModels, modelsLoading,
}: {
  value: string;
  onChange: (v: string) => void;
  localModels: string[];
  modelsLoading: boolean;
}) {
  // ALL locally installed models are shown — the user knows which ones support vision
  const installedOpts = localModels.map((m) => ({
    value: m,
    label: `✓ ${m}`,
  }));

  // Pullable suggestions — only those not already installed
  const pullableOpts = PULLABLE_VISION_MODELS.filter(
    (k) => !localModels.some((m) => m === k.name || m.startsWith(k.name.split(":")[0] + ":"))
  ).map((k) => ({ value: k.name, label: `↓ ${k.label}` }));

  return (
    <div className="flex gap-2 items-center">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        style={{ ...inputStyle, flex: 1, cursor: "pointer" }}>
        <option value="">Disabled (no screenshots)</option>

        {installedOpts.length > 0 && (
          <optgroup label="— Installed models —">
            {installedOpts.map((o) => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </optgroup>
        )}

        {pullableOpts.length > 0 && (
          <optgroup label="— Available to pull —">
            {pullableOpts.map((o) => (
              <option key={o.value} value={o.value}>{o.label}</option>
            ))}
          </optgroup>
        )}

        {/* Keep a saved value visible even if Ollama is offline */}
        {value && !installedOpts.some((o) => o.value === value) &&
                  !pullableOpts.some((o) => o.value === value) && (
          <option value={value}>{value}</option>
        )}
      </select>
      {modelsLoading && (
        <span className="text-xs" style={{ color: "var(--color-muted)", flexShrink: 0 }}>
          loading…
        </span>
      )}
    </div>
  );
}

function VisionTest({ url, model }: { url: string; model: string }) {
  const [result, setResult] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const run = async () => {
    setRunning(true);
    setResult(null);
    try {
      const desc = await invoke<string>("test_vision", { url, model });
      setResult("✓ " + desc);
    } catch (e: unknown) {
      setResult("✗ " + String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <button
        onClick={run}
        disabled={running}
        className="text-xs px-2 py-1 rounded self-start"
        style={{
          background: "var(--color-surface)",
          border: "1px solid var(--color-border)",
          color: running ? "var(--color-muted)" : "var(--color-accent)",
          cursor: running ? "not-allowed" : "pointer",
        }}
      >
        {running ? "Testing…" : "Test vision now"}
      </button>
      {result && (
        <p className="text-xs break-all" style={{
          color: result.startsWith("✓") ? "#4ade80" : "#f87171",
        }}>
          {result}
        </p>
      )}
    </div>
  );
}

// ── Settings component ─────────────────────────────────────────

export default function Settings() {
  const [ollamaUrl, setOllamaUrl] = useState("http://localhost:11434");
  const [ollamaModel, setOllamaModel] = useState("gemma3:4b");
  const [visionModel, setVisionModel] = useState("");
  const [notesDir, setNotesDir] = useState("");
  const [blockedApps, setBlockedApps] = useState("");
  const [watchWindow, setWatchWindow] = useState(true);
  const [watchClipboard, setWatchClipboard] = useState(true);
  const [watchFilesystem, setWatchFilesystem] = useState(true);
  const [watchComms, setWatchComms] = useState(false);
  const [dwellSecs, setDwellSecs] = useState(30);
  const [idleTimeoutSecs, setIdleTimeoutSecs] = useState(120);

  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);

  const [ollamaStatus, setOllamaStatus] = useState<"idle" | "checking" | "ok" | "fail">("idle");
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [toast, setToast] = useState<string | null>(null);

  // Load config on mount, then immediately fetch available models
  useEffect(() => {
    invoke<any>("load_config").then((cfg) => {
      if (!cfg) return;
      const url = cfg.ollama_url ?? "http://localhost:11434";
      setOllamaUrl(url);
      setOllamaModel(cfg.ollama_model ?? "");
      setVisionModel(cfg.vision_model ?? "");
      setNotesDir(cfg.notes_dir ?? "");
      setDwellSecs(cfg.min_dwell_secs ?? 30);
      setIdleTimeoutSecs(cfg.session_idle_timeout_secs ?? 120);
      setWatchComms(cfg.watch_communication_apps ?? false);
      setBlockedApps((cfg.blocked_apps ?? []).join("\n"));
      // Fetch models using the loaded URL
      fetchModels(url, cfg.ollama_model ?? "");
    }).catch(() => {
      setNotesDir("~/Documents/LogR");
    });
  }, []);

  async function fetchModels(url: string, currentModel?: string) {
    setModelsLoading(true);
    setModelsError(null);
    try {
      const models = await invoke<string[]>("list_ollama_models", { url });
      setAvailableModels(models);
      // If current model isn't in the list, auto-select the first one
      if (models.length > 0) {
        const target = currentModel ?? ollamaModel;
        if (!models.includes(target)) {
          setOllamaModel(models[0]);
        }
      }
    } catch (e: any) {
      setModelsError(String(e));
      setAvailableModels([]);
    } finally {
      setModelsLoading(false);
    }
  }

  function showToast(msg: string) {
    setToast(msg);
    setTimeout(() => setToast(null), 3000);
  }

  async function handleTestConnection() {
    setOllamaStatus("checking");
    try {
      const ok = await invoke<boolean>("check_ollama", { url: ollamaUrl });
      setOllamaStatus(ok ? "ok" : "fail");
      if (ok) fetchModels(ollamaUrl);
    } catch {
      setOllamaStatus("fail");
    }
  }

  async function handleOpenNotes() {
    const dir = notesDir || "~/Documents/LogR";
    try {
      await openPath(dir);
    } catch (e) {
      showToast("Could not open folder: " + e);
    }
  }

  async function handleSave() {
    setSaveStatus("saving");
    try {
      await invoke("save_config", {
        config: {
          ollama_url: ollamaUrl,
          ollama_model: ollamaModel,
          vision_model: visionModel,
          notes_dir: notesDir,
          min_dwell_secs: dwellSecs,
          watch_communication_apps: watchComms,
          blocked_apps: blockedApps.split("\n").map((s) => s.trim()).filter(Boolean),
          session_idle_timeout_secs: idleTimeoutSecs,
          watch_dirs: [],
        },
      });
      setSaveStatus("saved");
      setTimeout(() => setSaveStatus("idle"), 2000);
    } catch (e) {
      setSaveStatus("error");
      showToast("Save failed: " + e);
    }
  }

  async function handleClearNotes() {
    if (!confirm("Delete all notes in\n" + notesDir + "\n\nThis cannot be undone.")) return;
    try {
      const count = await invoke<number>("clear_notes", { notesDir: notesDir || "" });
      showToast(`Cleared ${count} item${count !== 1 ? "s" : ""} from notes folder`);
    } catch (e) {
      showToast("Failed to clear notes: " + e);
    }
  }

  async function handleResetConfig() {
    if (!confirm("Reset all settings to defaults?\n\nThis cannot be undone.")) return;
    try {
      await invoke("reset_config");
      showToast("Config reset — restart LogR to apply defaults");
    } catch (e) {
      showToast("Failed to reset config: " + e);
    }
  }

  const ollamaStatusColor = { idle: "var(--color-muted)", checking: "#f59e0b", ok: "#22c55e", fail: "#ef4444" }[ollamaStatus];
  const ollamaStatusText = { idle: "Test Connection", checking: "Checking…", ok: "Connected ✓", fail: "Unreachable ✗" }[ollamaStatus];

  return (
    <div className="flex flex-col h-screen"
      style={{ background: "var(--color-bg)", border: "1px solid var(--color-border)" }}>

      {/* Title bar */}
      <div className="flex items-center justify-between px-3 py-2 shrink-0"
        style={{ borderBottom: "1px solid var(--color-border)" }}
        data-tauri-drag-region>
        <span className="text-xs font-semibold tracking-widest uppercase" style={{ color: "var(--color-muted)" }}>
          Settings
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

      {/* Scrollable content */}
      <div className="flex-1 overflow-y-auto px-4 py-4" style={{ scrollbarWidth: "none" }}>

        <Section title="LLM">
          <Field label="Ollama URL">
            <input style={inputStyle} value={ollamaUrl} onChange={(e) => { setOllamaUrl(e.target.value); setOllamaStatus("idle"); }} />
          </Field>
          <Field label="Model">
            <div className="flex gap-2 items-center">
              {availableModels.length > 0 ? (
                <select
                  value={ollamaModel}
                  onChange={(e) => setOllamaModel(e.target.value)}
                  style={{ ...inputStyle, flex: 1, cursor: "pointer" }}>
                  {availableModels.map((m) => (
                    <option key={m} value={m}>{m}</option>
                  ))}
                </select>
              ) : (
                <input
                  style={{ ...inputStyle, flex: 1 }}
                  value={ollamaModel}
                  onChange={(e) => setOllamaModel(e.target.value)}
                  placeholder={modelsError ? "gemma3:4b (Ollama offline)" : "gemma3:4b"} />
              )}
              <button
                onClick={() => fetchModels(ollamaUrl)}
                disabled={modelsLoading}
                title="Refresh model list"
                style={{
                  background: "var(--color-border)", border: "1px solid var(--color-border)",
                  borderRadius: 6, padding: "5px 8px", cursor: modelsLoading ? "wait" : "pointer",
                  color: modelsLoading ? "var(--color-muted)" : "var(--color-accent)", flexShrink: 0,
                }}>
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"
                  style={{ display: "block", animation: modelsLoading ? "spin 1s linear infinite" : "none" }}>
                  <path d="M23 4v6h-6" /><path d="M1 20v-6h6" />
                  <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
                </svg>
              </button>
            </div>
            {modelsError && (
              <span className="text-xs mt-0.5" style={{ color: "#ef4444" }}>{modelsError}</span>
            )}
            {availableModels.length === 0 && !modelsError && !modelsLoading && (
              <span className="text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>
                Connect to Ollama to pick from installed models
              </span>
            )}
          </Field>
          <button
            onClick={handleTestConnection}
            disabled={ollamaStatus === "checking"}
            className="text-xs px-3 py-1.5 rounded self-start"
            style={{ background: "var(--color-border)", color: ollamaStatusColor, border: `1px solid ${ollamaStatusColor}`, cursor: ollamaStatus === "checking" ? "wait" : "pointer" }}>
            {ollamaStatusText}
          </button>

          <Field label="Vision model (screenshot descriptions)">
            <VisionModelPicker
              value={visionModel}
              onChange={setVisionModel}
              localModels={availableModels}
              modelsLoading={modelsLoading}
            />
            <span className="text-xs mt-1" style={{ color: "var(--color-muted)" }}>
              {visionModel
                ? `Screenshots will be described by ${visionModel} — images are never saved to disk.`
                : "Disabled — no screenshots will be taken."}
            </span>
            {visionModel && <VisionTest url={ollamaUrl} model={visionModel} />}
          </Field>
        </Section>

        <Section title="Watching">
          <ToggleRow label="Window focus" value={watchWindow} onChange={setWatchWindow} />
          <ToggleRow label="Clipboard changes" value={watchClipboard} onChange={setWatchClipboard} />
          <ToggleRow label="File edits" value={watchFilesystem} onChange={setWatchFilesystem} />
          <ToggleRow label="Communication apps (Slack, Discord)" value={watchComms} onChange={setWatchComms} />
          <Field label={`Minimum dwell time: ${dwellSecs}s`}>
            <input type="range" min={10} max={120} step={5} value={dwellSecs}
              onChange={(e) => setDwellSecs(Number(e.target.value))}
              style={{ accentColor: "var(--color-accent)", width: "100%" }} />
          </Field>
          <Field label={`Idle timeout (write note after): ${idleTimeoutSecs >= 60 ? `${Math.round(idleTimeoutSecs / 60)} min` : `${idleTimeoutSecs}s`}`}>
            <input type="range" min={30} max={600} step={30} value={idleTimeoutSecs}
              onChange={(e) => setIdleTimeoutSecs(Number(e.target.value))}
              style={{ accentColor: "var(--color-accent)", width: "100%" }} />
            <div className="flex justify-between text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>
              <span>30s</span>
              <span>5 min</span>
              <span>10 min</span>
            </div>
          </Field>
        </Section>

        <Section title="Notes">
          <Field label="Notes directory">
            <input style={inputStyle} value={notesDir} onChange={(e) => setNotesDir(e.target.value)} placeholder="~/Documents/LogR" />
          </Field>
          <button onClick={handleOpenNotes}
            className="text-xs px-3 py-1.5 rounded self-start"
            style={{ background: "var(--color-border)", color: "#e5e7eb", border: "1px solid var(--color-border)", cursor: "pointer" }}>
            Open in Explorer
          </button>
        </Section>

        <Section title="Privacy">
          <Field label="Additional blocked apps (one per line)">
            <textarea rows={3} value={blockedApps} onChange={(e) => setBlockedApps(e.target.value)}
              style={{ ...inputStyle, resize: "none", fontFamily: "monospace" }}
              placeholder={"Spotify\nSlack\n..."} />
          </Field>
        </Section>

        <Section title="Danger Zone">
          <button onClick={handleClearNotes}
            className="text-xs px-3 py-1.5 rounded self-start"
            style={{ background: "#1f0000", color: "#ef4444", border: "1px solid #7f1d1d", cursor: "pointer" }}>
            Clear All Notes
          </button>
          <button onClick={handleResetConfig}
            className="text-xs px-3 py-1.5 rounded self-start"
            style={{ background: "#1f0000", color: "#ef4444", border: "1px solid #7f1d1d", cursor: "pointer" }}>
            Reset Config to Defaults
          </button>
        </Section>
      </div>

      {/* Toast */}
      {toast && (
        <div className="absolute bottom-14 left-4 right-4 text-xs px-3 py-2 rounded"
          style={{ background: "var(--color-surface)", border: "1px solid var(--color-border)", color: "#e5e7eb" }}>
          {toast}
        </div>
      )}

      {/* Save footer */}
      <div className="px-4 py-3 shrink-0 flex justify-end"
        style={{ borderTop: "1px solid var(--color-border)" }}>
        <button onClick={handleSave}
          disabled={saveStatus === "saving"}
          className="text-sm px-4 py-1.5 rounded font-medium"
          style={{
            background: saveStatus === "saved" ? "#22c55e" : saveStatus === "error" ? "#ef4444" : "var(--color-accent)",
            color: "#0F0F0F", cursor: saveStatus === "saving" ? "wait" : "pointer",
            transition: "background 0.2s",
          }}>
          {saveStatus === "saving" ? "Saving…" : saveStatus === "saved" ? "Saved ✓" : saveStatus === "error" ? "Error ✗" : "Save"}
        </button>
      </div>
    </div>
  );
}
