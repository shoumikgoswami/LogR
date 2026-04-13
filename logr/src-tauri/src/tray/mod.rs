use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime,
};
use tauri_plugin_opener::OpenerExt;

use crate::state::FlushHandle;

/// Build a 32×32 RGBA tray icon: cyan eye ring + pupil dot on transparent bg.
pub fn make_tray_icon() -> Image<'static> {
    const SIZE: u32 = 32;
    const CX: f32 = 15.5;
    const CY: f32 = 15.5;

    // Accent cyan #22D3EE  →  R=34 G=211 B=238
    const R: u8 = 34;
    const G: u8 = 211;
    const B: u8 = 238;

    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];

    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - CX;
            let dy = y as f32 - CY;
            let dist = (dx * dx + dy * dy).sqrt();

            let idx = ((y * SIZE + x) * 4) as usize;

            // Outer eye ring  (radius 11–13)
            if dist >= 10.5 && dist <= 13.0 {
                let alpha = smooth_alpha(dist, 10.5, 13.0);
                set_pixel(&mut pixels, idx, R, G, B, alpha);
            }
            // Iris  (radius 5–7)
            else if dist >= 5.0 && dist <= 7.5 {
                let alpha = smooth_alpha(dist, 5.0, 7.5);
                set_pixel(&mut pixels, idx, R, G, B, alpha);
            }
            // Pupil dot  (radius 0–2.5)
            else if dist <= 2.5 {
                let alpha = smooth_alpha(dist, 0.0, 2.5);
                set_pixel(&mut pixels, idx, R, G, B, alpha);
            }
        }
    }

    Image::new_owned(pixels, SIZE, SIZE)
}

#[inline]
fn smooth_alpha(dist: f32, inner: f32, outer: f32) -> u8 {
    let mid = (inner + outer) / 2.0;
    let half = (outer - inner) / 2.0;
    let t = 1.0 - ((dist - mid) / half).abs().min(1.0);
    (t * 255.0) as u8
}

#[inline]
fn set_pixel(pixels: &mut [u8], idx: usize, r: u8, g: u8, b: u8, a: u8) {
    pixels[idx] = r;
    pixels[idx + 1] = g;
    pixels[idx + 2] = b;
    pixels[idx + 3] = a;
}

pub fn setup_tray<R: Runtime>(app: &tauri::AppHandle<R>) -> tauri::Result<()> {
    let separator = PredefinedMenuItem::separator(app)?;

    let open_dashboard = MenuItem::with_id(app, "open_dashboard", "Open Dashboard", true, None::<&str>)?;
    let pause_watching = MenuItem::with_id(app, "pause_watching", "⏸ Pause Watching", true, None::<&str>)?;
    let flush_session = MenuItem::with_id(app, "flush_session", "⚡ Flush Session Now", true, None::<&str>)?;
    let open_notes = MenuItem::with_id(app, "open_notes", "📁 Open Notes Folder", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "⚙ Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "✕ Quit", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &open_dashboard,
            &separator,
            &pause_watching,
            &flush_session,
            &open_notes,
            &separator,
            &settings,
            &separator,
            &quit,
        ],
    )?;

    let icon = make_tray_icon();

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("LogR — Watching")
        .icon(icon)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_dashboard" => {
                show_window(app, "dashboard");
            }
            "settings" => {
                show_window(app, "settings");
            }
            "open_notes" => {
                let notes_dir = dirs::document_dir()
                    .unwrap_or_default()
                    .join("LogR");
                std::fs::create_dir_all(&notes_dir).ok();
                if let Some(path_str) = notes_dir.to_str() {
                    if let Err(e) = app.opener().open_path(path_str, None::<&str>) {
                        tracing::error!("Failed to open notes folder: {}", e);
                    }
                }
            }
            "quit" => {
                app.exit(0);
            }
            "flush_session" => {
                if let Some(handle) = app.try_state::<FlushHandle>() {
                    if let Ok(guard) = handle.0.lock() {
                        if let Some(tx) = guard.as_ref() {
                            let _ = tx.try_send(());
                        }
                    }
                }
            }
            "pause_watching" => {
                tracing::info!("Pause/resume watching toggled");
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    show_window(tray.app_handle(), "dashboard");
                }
                TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => {
                    show_window(tray.app_handle(), "settings");
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}

fn show_window<R: Runtime>(app: &tauri::AppHandle<R>, label: &str) {
    if let Some(window) = app.get_webview_window(label) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
