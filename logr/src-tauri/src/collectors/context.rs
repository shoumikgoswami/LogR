/// Platform-level context extraction: browser URLs, terminal commands, etc.
/// All functions are best-effort — they return None rather than panic.

use sysinfo::{Pid, System};

/// Given a focused process ID and app name, try to extract rich context.
/// Returns a human-readable string describing what the user is actually doing.
pub fn get_app_context(process_id: u64, app_lower: &str) -> Option<String> {
    if is_browser(app_lower) {
        get_browser_url(process_id).or_else(|| None)
    } else if is_terminal(app_lower) {
        get_terminal_command(process_id)
    } else {
        None
    }
}

fn is_browser(app_lower: &str) -> bool {
    ["chrome", "firefox", "safari", "arc", "brave", "edge", "opera"]
        .iter()
        .any(|e| app_lower.contains(e))
}

fn is_terminal(app_lower: &str) -> bool {
    ["terminal", "iterm", "warp", "kitty", "alacritty", "hyper",
     "cmd", "powershell", "wezterm", "conhost", "windowsterminal"]
        .iter()
        .any(|e| app_lower.contains(e))
}

// ── Browser URL ───────────────────────────────────────────────────────────────

/// Get the URL from a focused browser window using Windows UI Automation.
/// Only compiled on Windows; returns None on other platforms or on any failure.
#[cfg(target_os = "windows")]
fn get_browser_url(process_id: u64) -> Option<String> {
    use windows::{
        core::BSTR,
        Win32::{
            Foundation::HWND,
            System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED},
            UI::{
                Accessibility::{
                    CUIAutomation, IUIAutomation, IUIAutomationValuePattern,
                    TreeScope_Subtree, UIA_EditControlTypeId, UIA_ValuePatternId,
                    UIA_ControlTypePropertyId, PropertyConditionFlags_None,
                },
                WindowsAndMessaging::GetForegroundWindow,
            },
        },
    };
    use windows::core::Interface;

    unsafe {
        // COM must be initialized per-thread (we're in spawn_blocking, so it's a dedicated thread)
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL).ok()?;

        // Get the foreground window and confirm it belongs to our process
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0 == 0 as _ {
            return None;
        }

        let root = automation.ElementFromHandle(hwnd).ok()?;

        // Check this element belongs to the expected process
        let pid = root.CurrentProcessId().ok()?;
        if pid as u64 != process_id {
            return None;
        }

        // Find the address bar: an Edit control somewhere in the window tree
        // Chrome/Edge/Brave/Firefox all use Edit controls for the address bar
        let cond = automation.CreatePropertyConditionEx(
            UIA_ControlTypePropertyId,
            &windows::core::VARIANT::from(UIA_EditControlTypeId.0 as i32),
            PropertyConditionFlags_None,
        ).ok()?;

        let element = root.FindFirst(TreeScope_Subtree, &cond).ok()?;

        // Read the value via the Value pattern
        let pattern = element.GetCurrentPattern(UIA_ValuePatternId).ok()?;
        let value_pattern: IUIAutomationValuePattern = pattern.cast().ok()?;
        let value: BSTR = value_pattern.CurrentValue().ok()?;
        let url = value.to_string();

        if url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://") {
            Some(url)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn get_browser_url(_process_id: u64) -> Option<String> {
    None
}

// ── Terminal command ──────────────────────────────────────────────────────────

/// Find the active command running inside a terminal process (shell, running program).
/// Uses sysinfo to walk child processes of the terminal and return the leaf command.
fn get_terminal_command(terminal_pid: u64) -> Option<String> {
    let mut sys = System::new();
    sys.refresh_processes();

    let terminal_pid = Pid::from(terminal_pid as usize);

    // Collect all child processes of the terminal
    let mut children: Vec<_> = sys
        .processes()
        .values()
        .filter(|p| p.parent() == Some(terminal_pid))
        .collect();

    if children.is_empty() {
        return None;
    }

    // Sort by PID descending to prefer the most recently spawned child
    children.sort_by_key(|p| std::cmp::Reverse(p.pid()));

    // Skip shell processes — we want what's running inside the shell
    let shells = ["cmd.exe", "powershell.exe", "pwsh.exe", "bash", "zsh", "sh", "fish", "nu"];

    // First try a non-shell child (an actual running command)
    for child in &children {
        let name = child.name().to_lowercase();
        if !shells.iter().any(|s| name.contains(s)) {
            let cmd = child.cmd()
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(" ");
            if !cmd.is_empty() {
                return Some(format!("Running: {}", truncate(&cmd, 120)));
            }
        }
    }

    // Fall back to the shell itself with its CWD
    if let Some(shell) = children.first() {
        let cwd = shell.cwd()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let shell_name = shell.name().to_string();
        if !cwd.is_empty() {
            return Some(format!("{} in {}", shell_name, shorten_path(&cwd)));
        }
        return Some(shell_name);
    }

    None
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Shorten a long path by keeping the last 2–3 components.
fn shorten_path(path: &str) -> String {
    let sep = if path.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = path.split(sep).filter(|s| !s.is_empty()).collect();
    if parts.len() <= 3 {
        path.to_string()
    } else {
        format!("…/{}", parts[parts.len() - 2..].join("/"))
    }
}
