/// Daily summary generator.
///
/// On startup, scans past day-folders in the notes directory. Any folder that
/// has session notes but no `daily_summary.md` gets a summary generated via
/// the configured LLM provider and written back into that folder.
///
/// Runs silently in the background — failures are logged as warnings, never
/// propagated to the main pipeline.

use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use tauri::Manager;

use crate::state::{DriftlogConfig, SharedStats};
use crate::synthesis::ollama::{OllamaClient, OllamaConfig};
use crate::synthesis::openrouter::OpenRouterClient;

const MAX_NOTES_CHARS: usize = 12_000;
const MAX_DAYS_TO_PROCESS: usize = 7;
const SUMMARY_FILENAME: &str = "daily_summary.md";

// ── Public entry point ────────────────────────────────────────────────────────

/// Find past-day folders that have notes but no summary and generate one for each.
pub async fn maybe_generate_daily_summaries(
    notes_dir: &Path,
    config: &DriftlogConfig,
    app: &tauri::AppHandle,
) {
    let today = Local::now().date_naive();

    let pending = match find_pending_days(notes_dir, today) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("[daily_summary] failed to scan notes dir: {}", e);
            return;
        }
    };

    if pending.is_empty() {
        tracing::debug!("[daily_summary] all past days already have summaries");
        return;
    }

    tracing::info!("[daily_summary] {} day(s) need summaries", pending.len());

    for (date, day_dir) in pending.into_iter().take(MAX_DAYS_TO_PROCESS) {
        match generate_for_day(&date, &day_dir, config).await {
            Ok(path) => {
                tracing::info!("[daily_summary] wrote {}", path.display());
                // Update dashboard "last note" so the user sees it
                if let Some(stats) = app.try_state::<SharedStats>() {
                    if let Ok(mut guard) = stats.0.lock() {
                        guard.last_note_path = Some(path.to_string_lossy().into_owned());
                        guard.total_notes += 1;
                    }
                }
            }
            Err(e) => tracing::warn!("[daily_summary] {} failed: {}", date, e),
        }
    }
}

/// Generate (or regenerate) the daily summary for a specific date string ("YYYY-MM-DD").
/// If date is empty, defaults to yesterday. Returns the path of the written file.
pub async fn generate_for_date(
    date_str: &str,
    notes_dir: &Path,
    config: &DriftlogConfig,
) -> Result<PathBuf, String> {
    let today = Local::now().date_naive();
    let date: NaiveDate = if date_str.trim().is_empty() {
        today
            .pred_opt()
            .ok_or_else(|| "could not compute yesterday".to_string())?
    } else {
        NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d")
            .map_err(|e| format!("invalid date '{}': {}", date_str, e))?
    };

    if date >= today {
        return Err("cannot summarise today — the day is still in progress".into());
    }

    let day_dir = notes_dir.join(date.format("%Y-%m-%d").to_string());
    if !day_dir.exists() {
        return Err(format!("no notes folder for {}", date));
    }

    generate_for_day(&date, &day_dir, config).await
}

// ── Core logic ────────────────────────────────────────────────────────────────

/// Returns a sorted list of (date, path) pairs for past day-folders that have
/// session notes but no daily_summary.md yet.
fn find_pending_days(
    notes_dir: &Path,
    today: NaiveDate,
) -> Result<Vec<(NaiveDate, PathBuf)>, String> {
    if !notes_dir.exists() {
        return Ok(vec![]);
    }

    let mut pending: Vec<(NaiveDate, PathBuf)> = std::fs::read_dir(notes_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = path.file_name()?.to_str()?;
            let date = NaiveDate::parse_from_str(name, "%Y-%m-%d").ok()?;

            // Skip today and future dates
            if date >= today {
                return None;
            }

            // Skip if summary already exists
            if path.join(SUMMARY_FILENAME).exists() {
                return None;
            }

            // Skip if there are no session notes
            let has_notes = std::fs::read_dir(&path).ok()?.any(|e| {
                let e = match e {
                    Ok(e) => e,
                    Err(_) => return false,
                };
                let p = e.path();
                p.extension().and_then(|x| x.to_str()) == Some("md")
                    && p.file_name().and_then(|x| x.to_str()) != Some(SUMMARY_FILENAME)
                    && p.file_name().and_then(|x| x.to_str()) != Some("index.md")
            });

            if has_notes {
                Some((date, path))
            } else {
                None
            }
        })
        .collect();

    // Oldest first so we process in order
    pending.sort_by_key(|(d, _)| *d);
    Ok(pending)
}

async fn generate_for_day(
    date: &NaiveDate,
    day_dir: &Path,
    config: &DriftlogConfig,
) -> Result<PathBuf, String> {
    let date_str = date.format("%Y-%m-%d").to_string();
    tracing::info!("[daily_summary] generating summary for {}", date_str);

    // Collect note contents
    let (notes_content, note_count) = collect_notes(day_dir)?;

    if note_count == 0 {
        return Err(format!("no notes found in {}", day_dir.display()));
    }

    // Build the prompt
    let prompt = build_summary_prompt(&date_str, &notes_content);

    // Call the LLM — fall back to structured concatenation if unavailable
    let body = match call_llm(config, &prompt).await {
        Ok(text) => {
            tracing::debug!("[daily_summary] LLM returned {} chars", text.len());
            text
        }
        Err(e) => {
            tracing::warn!("[daily_summary] LLM unavailable ({}), writing fallback summary", e);
            build_fallback_summary(&date_str, day_dir)?
        }
    };

    // Write the summary file
    let summary_path = day_dir.join(SUMMARY_FILENAME);
    let frontmatter = format!(
        "---\ndate: {}\ntype: daily-summary\nnote_count: {}\ngenerated_at: {}\nsynthesized: true\n---\n\n",
        date_str,
        note_count,
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ"),
    );
    let content = format!("{}{}", frontmatter, body);
    std::fs::write(&summary_path, &content).map_err(|e| e.to_string())?;

    Ok(summary_path)
}

/// Read all session `.md` files from a day folder, strip frontmatter,
/// and concatenate up to MAX_NOTES_CHARS characters.
fn collect_notes(day_dir: &Path) -> Result<(String, usize), String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(day_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| {
            let e = e.ok()?;
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_owned();
            if p.extension()?.to_str() != Some("md") {
                return None;
            }
            if name == SUMMARY_FILENAME || name == "index.md" {
                return None;
            }
            Some(p)
        })
        .collect();

    // Sort by filename (HH-MM prefix) → chronological order
    entries.sort();

    let mut combined = String::new();
    let mut count = 0usize;

    for path in &entries {
        let raw = std::fs::read_to_string(path).unwrap_or_default();
        let body = strip_frontmatter(&raw);
        if body.trim().is_empty() {
            continue;
        }
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("note");
        combined.push_str(&format!("\n\n### {}\n{}", filename, body));
        count += 1;

        if combined.len() >= MAX_NOTES_CHARS {
            combined.truncate(MAX_NOTES_CHARS);
            combined.push_str("\n\n*[remaining notes truncated for length]*");
            break;
        }
    }

    Ok((combined, count))
}

/// Strip YAML frontmatter (the `---` … `---` block at the top).
fn strip_frontmatter(content: &str) -> &str {
    let content = content.trim_start();
    if !content.starts_with("---") {
        return content;
    }
    // Find the closing `---`
    let after_first = &content[3..];
    if let Some(end) = after_first.find("\n---") {
        let after_fm = &after_first[end + 4..]; // skip \n---
        // skip the optional newline right after the closing ---
        after_fm.trim_start_matches('\n')
    } else {
        content
    }
}

fn build_summary_prompt(date: &str, notes_content: &str) -> String {
    format!(
        r#"You are a personal knowledge logger. Below are all the notes captured on {date}.

Your task: write a **detailed daily summary** in structured markdown.

Rules:
- Organise by project/theme (not chronologically)
- Highlight key accomplishments, decisions, and ongoing work
- Be specific: name actual files, apps, URLs, and topics mentioned in the notes
- Include a "Work in Progress" section if anything looks unfinished
- Maximum 500 words
- Start with a bold heading: **Daily Summary — {date}**
- Output only markdown, no preamble

--- Notes from {date} ---
{notes}
--- End ---

Write the daily summary now."#,
        date = date,
        notes = notes_content,
    )
}

async fn call_llm(config: &DriftlogConfig, prompt: &str) -> Result<String, String> {
    if config.provider == "openrouter" {
        if config.openrouter_api_key.trim().is_empty() {
            return Err("OpenRouter API key not set".into());
        }
        let client = OpenRouterClient::new(
            config.openrouter_api_key.clone(),
            config.openrouter_model.clone(),
        );
        client.complete(prompt, &config.openrouter_model).await
    } else {
        let client = OllamaClient::new(OllamaConfig {
            model: config.ollama_model.clone(),
            base_url: config.ollama_url.clone(),
            temperature: 0.3,
            max_tokens: 1024,
        });
        client.complete(prompt, &config.ollama_model).await
    }
}

/// Fallback: build a simple structured summary from note headings without LLM.
fn build_fallback_summary(date: &str, day_dir: &Path) -> Result<String, String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(day_dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| {
            let e = e.ok()?;
            let p = e.path();
            let name = p.file_name()?.to_str()?.to_owned();
            if p.extension()?.to_str() != Some("md") { return None; }
            if name == SUMMARY_FILENAME || name == "index.md" { return None; }
            Some(p)
        })
        .collect();
    entries.sort();

    let mut lines = vec![
        format!("**Daily Summary — {}**\n", date),
        "*Generated from session headings — AI summary unavailable at write time.*\n".into(),
    ];

    for path in &entries {
        let raw = std::fs::read_to_string(path).unwrap_or_default();
        let body = strip_frontmatter(&raw);
        // Extract the first bold heading line from the note body
        let heading = body
            .lines()
            .find(|l| l.trim_start().starts_with("**"))
            .unwrap_or("(no heading)")
            .trim()
            .to_string();
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("note");
        // HH-MM from filename prefix
        let time = filename.get(..5).unwrap_or("??:??").replace('-', ":");
        lines.push(format!("- **{}** — {}", time, heading));
    }

    Ok(lines.join("\n"))
}
