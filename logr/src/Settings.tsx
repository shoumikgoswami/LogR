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

// ── Active provider selector ───────────────────────────────────

function ProviderSelector({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const options = [
    { id: "ollama",      label: "Ollama",      sub: "local" },
    { id: "openrouter",  label: "OpenRouter",  sub: "cloud" },
  ];
  return (
    <div className="flex gap-2">
      {options.map((o) => {
        const active = value === o.id;
        return (
          <button
            key={o.id}
            onClick={() => onChange(o.id)}
            className="flex-1 flex items-center gap-2 px-3 py-2 rounded-lg text-left"
            style={{
              background: active ? "rgba(34,211,238,0.08)" : "var(--color-surface)",
              border: `1.5px solid ${active ? "var(--color-accent)" : "var(--color-border)"}`,
              cursor: "pointer",
            }}
          >
            {/* radio dot */}
            <div style={{
              width: 14, height: 14, borderRadius: "50%", flexShrink: 0,
              border: `2px solid ${active ? "var(--color-accent)" : "var(--color-border)"}`,
              background: active ? "var(--color-accent)" : "transparent",
              display: "flex", alignItems: "center", justifyContent: "center",
            }}>
              {active && <div style={{ width: 5, height: 5, borderRadius: "50%", background: "#0F0F0F" }} />}
            </div>
            <div>
              <div className="text-xs font-semibold" style={{ color: active ? "var(--color-accent)" : "#e5e7eb" }}>{o.label}</div>
              <div className="text-xs" style={{ color: "var(--color-muted)" }}>{o.sub}</div>
            </div>
          </button>
        );
      })}
    </div>
  );
}

// ── Vision model picker ────────────────────────────────────────

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
  const installedOpts = localModels.map((m) => ({ value: m, label: `✓ ${m}` }));
  const pullableOpts = PULLABLE_VISION_MODELS.filter(
    (k) => !localModels.some((m) => m === k.name || m.startsWith(k.name.split(":")[0] + ":"))
  ).map((k) => ({ value: k.name, label: `↓ ${k.label}` }));

  return (
    <div className="flex gap-2 items-center">
      <select value={value} onChange={(e) => onChange(e.target.value)}
        style={{ ...inputStyle, flex: 1, cursor: "pointer" }}>
        <option value="">Disabled (no screenshots)</option>
        {installedOpts.length > 0 && (
          <optgroup label="— Installed models —">
            {installedOpts.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </optgroup>
        )}
        {pullableOpts.length > 0 && (
          <optgroup label="— Available to pull —">
            {pullableOpts.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
          </optgroup>
        )}
        {value && !installedOpts.some((o) => o.value === value) && !pullableOpts.some((o) => o.value === value) && (
          <option value={value}>{value}</option>
        )}
      </select>
      {modelsLoading && (
        <span className="text-xs" style={{ color: "var(--color-muted)", flexShrink: 0 }}>loading…</span>
      )}
    </div>
  );
}

// ── OpenRouter vision model picker ────────────────────────────────────────────

// Multimodal models available on OpenRouter that support image input
const OR_VISION_MODELS = [
  { id: "google/gemini-2.0-flash-001",          label: "Gemini 2.0 Flash — fast, multimodal (recommended)" },
  { id: "google/gemini-flash-1.5-8b",           label: "Gemini Flash 1.5 8B — cheapest multimodal" },
  { id: "google/gemini-2.5-pro-preview-03-25",  label: "Gemini 2.5 Pro — powerful, multimodal" },
  { id: "anthropic/claude-3.5-haiku",           label: "Claude 3.5 Haiku — fast, multimodal" },
  { id: "anthropic/claude-3.5-sonnet",          label: "Claude 3.5 Sonnet — accurate, multimodal" },
  { id: "openai/gpt-4o-mini",                   label: "GPT-4o Mini — fast, multimodal" },
  { id: "openai/gpt-4o",                        label: "GPT-4o — accurate, multimodal" },
  { id: "meta-llama/llama-3.2-11b-vision-instruct", label: "Llama 3.2 Vision 11B — open source" },
  { id: "mistralai/pixtral-12b",                label: "Pixtral 12B — Mistral vision model" },
];

function OrVisionModelPicker({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  return (
    <div className="flex flex-col gap-1.5">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        style={{ ...inputStyle, cursor: "pointer" }}>
        <option value="">Disabled (no screenshots)</option>
        <optgroup label="— Multimodal models —">
          {OR_VISION_MODELS.map((m) => (
            <option key={m.id} value={m.id}>{m.id} — {m.label.split(" — ")[1]}</option>
          ))}
        </optgroup>
        {/* keep a saved value visible even if not in the list */}
        {value && !OR_VISION_MODELS.some((m) => m.id === value) && (
          <option value={value}>{value}</option>
        )}
      </select>
      <input
        style={{ ...inputStyle, fontSize: 12 }}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        placeholder="or type any multimodal model ID…"
      />
    </div>
  );
}

function VisionTest({ provider, ollamaUrl, visionModel, orApiKey }: {
  provider: string;
  ollamaUrl: string;
  visionModel: string;
  orApiKey: string;
}) {
  const [result, setResult] = useState<string | null>(null);
  const [running, setRunning] = useState(false);

  const run = async () => {
    setRunning(true);
    setResult(null);
    try {
      const desc = await invoke<string>("test_vision_with", {
        provider,
        ollamaUrl,
        visionModel,
        openrouterApiKey: orApiKey,
      });
      setResult("✓ " + desc);
    } catch (e: unknown) {
      setResult("✗ " + String(e));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="flex flex-col gap-1">
      <button onClick={run} disabled={running} className="text-xs px-2 py-1 rounded self-start"
        style={{
          background: "var(--color-surface)", border: "1px solid var(--color-border)",
          color: running ? "var(--color-muted)" : "var(--color-accent)",
          cursor: running ? "not-allowed" : "pointer",
        }}>
        {running ? "Testing…" : "Test vision now"}
      </button>
      {result && (
        <p className="text-xs break-all" style={{ color: result.startsWith("✓") ? "#4ade80" : "#f87171" }}>
          {result}
        </p>
      )}
    </div>
  );
}

// ── OpenRouter popular models (shown as suggestions when API key set) ──

const OR_POPULAR_MODELS = [
  "anthropic/claude-3.5-haiku",
  "anthropic/claude-3.5-sonnet",
  "google/gemini-2.0-flash-001",
  "google/gemini-2.5-pro-preview-03-25",
  "google/gemini-flash-1.5-8b",
  "meta-llama/llama-3.3-70b-instruct",
  "mistralai/mistral-nemo",
  "openai/gpt-4o-mini",
  "openai/gpt-4o",
  "qwen/qwen-2.5-72b-instruct",
];

// ── Settings component ─────────────────────────────────────────

export default function Settings() {
  // ── Provider ──────────────────────────────────────────────────
  const [provider, setProvider] = useState("ollama");

  // ── Ollama ────────────────────────────────────────────────────
  const [ollamaUrl, setOllamaUrl] = useState("http://localhost:11434");
  const [ollamaModel, setOllamaModel] = useState("gemma3:4b");
  const [visionModel, setVisionModel] = useState("");
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [ollamaStatus, setOllamaStatus] = useState<"idle" | "checking" | "ok" | "fail">("idle");

  // ── OpenRouter ────────────────────────────────────────────────
  const [orApiKey, setOrApiKey] = useState("");
  const [orModel, setOrModel] = useState("google/gemini-2.0-flash-001");
  const [orModels, setOrModels] = useState<string[]>([]);
  const [orModelsLoading, setOrModelsLoading] = useState(false);
  const [orModelsError, setOrModelsError] = useState<string | null>(null);
  const [orKeyRevealed, setOrKeyRevealed] = useState(false);

  // ── Shared ────────────────────────────────────────────────────
  const [notesDir, setNotesDir] = useState("");
  const [blockedApps, setBlockedApps] = useState("");
  const [watchWindow, setWatchWindow] = useState(true);
  const [watchClipboard, setWatchClipboard] = useState(true);
  const [watchFilesystem, setWatchFilesystem] = useState(true);
  const [watchComms, setWatchComms] = useState(false);
  const [dwellSecs, setDwellSecs] = useState(30);
  const [idleTimeoutSecs, setIdleTimeoutSecs] = useState(120);
  const [saveStatus, setSaveStatus] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [toast, setToast] = useState<string | null>(null);

  // Load config on mount
  useEffect(() => {
    invoke<any>("load_config").then((cfg) => {
      if (!cfg) return;
      const prov = cfg.provider ?? "ollama";
      setProvider(prov);
      const url = cfg.ollama_url ?? "http://localhost:11434";
      setOllamaUrl(url);
      setOllamaModel(cfg.ollama_model ?? "gemma3:4b");
      setVisionModel(cfg.vision_model ?? "");
      setOrApiKey(cfg.openrouter_api_key ?? "");
      setOrModel(cfg.openrouter_model ?? "google/gemini-flash-1.5");
      setNotesDir(cfg.notes_dir ?? "");
      setDwellSecs(cfg.min_dwell_secs ?? 30);
      setIdleTimeoutSecs(cfg.session_idle_timeout_secs ?? 120);
      setWatchComms(cfg.watch_communication_apps ?? false);
      setBlockedApps((cfg.blocked_apps ?? []).join("\n"));
      fetchOllamaModels(url, cfg.ollama_model ?? "");
    }).catch(() => {
      setNotesDir("~/Documents/LogR");
    });
  }, []);

  async function fetchOllamaModels(url: string, currentModel?: string) {
    setModelsLoading(true);
    setModelsError(null);
    try {
      const models = await invoke<string[]>("list_ollama_models", { url });
      setAvailableModels(models);
      // Only set to first model if nothing is saved yet
      if (models.length > 0 && !currentModel && !ollamaModel) {
        setOllamaModel(models[0]);
      }
    } catch (e: any) {
      setModelsError(String(e));
      setAvailableModels([]);
    } finally {
      setModelsLoading(false);
    }
  }

  async function fetchOrModels() {
    if (!orApiKey.trim()) {
      setOrModelsError("Enter your API key first");
      return;
    }
    setOrModelsLoading(true);
    setOrModelsError(null);
    try {
      const models = await invoke<string[]>("list_openrouter_models", { apiKey: orApiKey });
      setOrModels(models);
    } catch (e: any) {
      setOrModelsError(String(e));
    } finally {
      setOrModelsLoading(false);
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
      if (ok) fetchOllamaModels(ollamaUrl);
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
          provider,
          ollama_url: ollamaUrl,
          ollama_model: ollamaModel,
          openrouter_api_key: orApiKey,
          openrouter_model: orModel,
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
      // Trigger immediate provider status check so the dashboard reflects
      // the new config without waiting for the 10-second background tick.
      invoke("refresh_provider_status").catch(() => {});
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

  const [orStatus, setOrStatus] = useState<"idle" | "checking" | "ok" | "fail">("idle");
  const orStatusColor = { idle: "var(--color-muted)", checking: "#f59e0b", ok: "#22c55e", fail: "#ef4444" }[orStatus];
  const orStatusText = { idle: "Test Connection", checking: "Checking…", ok: "Connected ✓", fail: "Invalid key ✗" }[orStatus];

  async function handleTestOrConnection() {
    setOrStatus("checking");
    try {
      const ok = await invoke<boolean>("check_openrouter", { apiKey: orApiKey });
      setOrStatus(ok ? "ok" : "fail");
      if (ok) fetchOrModels();
    } catch {
      setOrStatus("fail");
    }
  }


  return (
    <div className="flex flex-col h-screen"
      style={{ background: "var(--color-bg)", border: "1px solid var(--color-border)" }}>

      {/* Title bar — mousedown starts native window drag; close button stops propagation */}
      <div className="flex items-center justify-between px-3 py-2 shrink-0"
        style={{ borderBottom: "1px solid var(--color-border)", cursor: "grab" }}
        onMouseDown={() => win.startDragging()}>
        <span className="text-xs font-semibold tracking-widest uppercase" style={{ color: "var(--color-muted)" }}>
          Settings
        </span>
        <button onClick={() => win.hide()}
          onMouseDown={(e) => e.stopPropagation()}
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

        {/* Active provider selector */}
        <Section title="Active Provider">
          <ProviderSelector value={provider} onChange={setProvider} />
          <span className="text-xs" style={{ color: "var(--color-muted)" }}>
            Configure both below — switch anytime by selecting above and saving.
          </span>
        </Section>

        {/* Ollama config — always editable */}
        <Section title="Ollama (local)">
          <Field label="URL">
            <input style={inputStyle} value={ollamaUrl}
              onChange={(e) => { setOllamaUrl(e.target.value); setOllamaStatus("idle"); }} />
          </Field>
          <Field label="Model">
            <div className="flex gap-2 items-center">
              <input
                list="ollama-models-list"
                style={{ ...inputStyle, flex: 1 }}
                value={ollamaModel}
                onChange={(e) => setOllamaModel(e.target.value)}
                placeholder="gemma3:4b"
              />
              <datalist id="ollama-models-list">
                {availableModels.map((m) => <option key={m} value={m} />)}
              </datalist>
              <button onClick={() => fetchOllamaModels(ollamaUrl)} disabled={modelsLoading}
                title="Refresh installed models"
                style={{
                  background: "var(--color-border)", border: "1px solid var(--color-border)",
                  borderRadius: 6, padding: "5px 8px", cursor: modelsLoading ? "wait" : "pointer",
                  color: modelsLoading ? "var(--color-muted)" : "var(--color-accent)", flexShrink: 0,
                }}>
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                  strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"
                  style={{ display: "block", animation: modelsLoading ? "spin 1s linear infinite" : "none" }}>
                  <path d="M23 4v6h-6" /><path d="M1 20v-6h6" />
                  <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
                </svg>
              </button>
            </div>
            {modelsError && <span className="text-xs mt-0.5" style={{ color: "#ef4444" }}>{modelsError}</span>}
            {availableModels.length > 0 && (
              <span className="text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>
                {availableModels.length} installed — type to filter or enter any model name
              </span>
            )}
            {availableModels.length === 0 && !modelsError && !modelsLoading && (
              <span className="text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>
                Offline — type any model name (e.g. gemma3:4b, llama3.2:3b)
              </span>
            )}
          </Field>
          <button onClick={handleTestConnection} disabled={ollamaStatus === "checking"}
            className="text-xs px-3 py-1.5 rounded self-start"
            style={{ background: "var(--color-border)", color: ollamaStatusColor, border: `1px solid ${ollamaStatusColor}`, cursor: ollamaStatus === "checking" ? "wait" : "pointer" }}>
            {ollamaStatusText}
          </button>
        </Section>

        {/* OpenRouter config — always editable */}
        <Section title="OpenRouter (cloud)">
          <Field label="API Key">
            <div className="flex gap-2 items-center">
              <input
                type={orKeyRevealed ? "text" : "password"}
                style={{ ...inputStyle, flex: 1, fontFamily: orKeyRevealed ? "monospace" : undefined }}
                value={orApiKey}
                onChange={(e) => setOrApiKey(e.target.value)}
                placeholder="sk-or-v1-…"
                autoComplete="off"
              />
              <button onClick={() => setOrKeyRevealed((v) => !v)}
                title={orKeyRevealed ? "Hide key" : "Show key"}
                style={{
                  background: "var(--color-border)", border: "1px solid var(--color-border)",
                  borderRadius: 6, padding: "5px 8px", cursor: "pointer",
                  color: "var(--color-muted)", flexShrink: 0,
                }}>
                {orKeyRevealed ? (
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24" />
                    <line x1="1" y1="1" x2="23" y2="23" />
                  </svg>
                ) : (
                  <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
                    <circle cx="12" cy="12" r="3" />
                  </svg>
                )}
              </button>
            </div>
            <span className="text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>
              Get a key at <span style={{ color: "var(--color-accent)" }}>openrouter.ai/keys</span>
            </span>
          </Field>
          <button onClick={handleTestOrConnection} disabled={orStatus === "checking"}
            className="text-xs px-3 py-1.5 rounded self-start"
            style={{ background: "var(--color-border)", color: orStatusColor, border: `1px solid ${orStatusColor}`, cursor: orStatus === "checking" ? "wait" : "pointer" }}>
            {orStatusText}
          </button>
          <Field label="Model">
            <div className="flex gap-2 items-center">
              <input
                list="or-models-list"
                style={{ ...inputStyle, flex: 1 }}
                value={orModel}
                onChange={(e) => setOrModel(e.target.value)}
                placeholder="google/gemini-2.0-flash-001"
              />
              <datalist id="or-models-list">
                {orModels.slice(0, 100).map((m) => <option key={m} value={m} />)}
              </datalist>
              <button onClick={fetchOrModels} disabled={orModelsLoading}
                title="Fetch full model list from OpenRouter"
                style={{
                  background: "var(--color-border)", border: "1px solid var(--color-border)",
                  borderRadius: 6, padding: "5px 8px", cursor: orModelsLoading ? "wait" : "pointer",
                  color: orModelsLoading ? "var(--color-muted)" : "var(--color-accent)", flexShrink: 0,
                }}>
                <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor"
                  strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round"
                  style={{ display: "block", animation: orModelsLoading ? "spin 1s linear infinite" : "none" }}>
                  <path d="M23 4v6h-6" /><path d="M1 20v-6h6" />
                  <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
                </svg>
              </button>
            </div>
            {orModelsError && <span className="text-xs mt-0.5" style={{ color: "#ef4444" }}>{orModelsError}</span>}
            <div className="flex flex-wrap gap-1 mt-1">
              {OR_POPULAR_MODELS.map((m) => (
                <button key={m} onClick={() => setOrModel(m)}
                  className="text-xs px-1.5 py-0.5 rounded"
                  style={{
                    background: orModel === m ? "var(--color-accent)" : "var(--color-border)",
                    color: orModel === m ? "#0F0F0F" : "#e5e7eb",
                    border: "1px solid var(--color-border)", cursor: "pointer",
                  }}>
                  {m.split("/")[1] ?? m}
                </button>
              ))}
            </div>
            {orModels.length === 0 && !orModelsError && (
              <span className="text-xs mt-0.5" style={{ color: "var(--color-muted)" }}>
                Click ↻ to load full model list, or type any model ID
              </span>
            )}
          </Field>
        </Section>

        {/* Vision — provider-aware */}
        <Section title="Vision (screenshot descriptions)">
          {provider === "ollama" ? (
            <Field label="Vision model">
              <VisionModelPicker
                value={visionModel}
                onChange={setVisionModel}
                localModels={availableModels}
                modelsLoading={modelsLoading}
              />
              <span className="text-xs mt-1" style={{ color: "var(--color-muted)" }}>
                {visionModel
                  ? `Screenshots sent to local Ollama (${visionModel}) — never stored on disk.`
                  : "Disabled — no screenshots will be taken."}
              </span>
              {visionModel && (
                <VisionTest provider="ollama" ollamaUrl={ollamaUrl} visionModel={visionModel} orApiKey="" />
              )}
            </Field>
          ) : (
            <Field label="Vision model">
              <OrVisionModelPicker value={visionModel} onChange={setVisionModel} />
              <span className="text-xs mt-1" style={{ color: "var(--color-muted)" }}>
                {visionModel
                  ? `Screenshots sent to OpenRouter (${visionModel}) — images are never stored on disk.`
                  : "Disabled — no screenshots will be taken."}
              </span>
              {visionModel && (
                <VisionTest provider="openrouter" ollamaUrl="" visionModel={visionModel} orApiKey={orApiKey} />
              )}
            </Field>
          )}
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
        <button onClick={handleSave} disabled={saveStatus === "saving"}
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
