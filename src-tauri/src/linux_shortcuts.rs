//! Wayland global-shortcut backend, using the
//! `org.freedesktop.portal.GlobalShortcuts` D-Bus portal via `ashpd`.
//!
//! Plain X11-style key-grabbing (what `tauri-plugin-global-shortcut` uses,
//! and what we use on Windows/macOS/X11-Linux) does not exist on Wayland by
//! design: Wayland's whole security model is built around NOT letting one
//! app silently listen to another app's keyboard input. The portal is the
//! sanctioned replacement, but it works differently in a few important
//! ways:
//!
//! - The app can only *request* shortcuts with a *preferred* trigger; the
//!   compositor/user has the final say (a system dialog appears the first
//!   time, and some desktops let the user reassign the actual key combo in
//!   their own settings).
//! - There's no cheap "register only while another key is held" the way
//!   `RegisterHotKey`/`XGrabKey` allow. All shortcuts are bound once for the
//!   whole session; we replicate the "only do something while the camera is
//!   held" behavior in software instead (see `handle_activated` below).
//! - Not every compositor implements this portal yet. GNOME (42+) and KDE
//!   Plasma (6+) do; minimal/tiling compositors often don't. When the portal
//!   is unavailable we log a clear error instead of silently doing nothing.
//!
//! This module intentionally re-implements the small state machine from
//! `camera_hold_handler`/`mock_toggle_handler` in `lib.rs` rather than
//! sharing code with them directly, since the two backends are driven by
//! fundamentally different mechanisms (synchronous OS callbacks vs. an
//! async D-Bus signal stream). The *behavior* is intended to match.

use crate::{Config, MODE_EVENT, OVERLAY_LABEL, ZOOM_EVENT};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use ashpd::desktop::Session;
use futures_util::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

const PAN_ACTIONS: [&str; 4] = ["pan_up", "pan_down", "pan_left", "pan_right"];

/// True if this process is running under a Wayland session (as opposed to
/// X11, where the existing tauri-plugin-global-shortcut path is used).
pub fn is_wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Spawns the portal-based shortcut listener in the background. Errors
/// (portal missing, D-Bus unavailable, etc.) are logged, not fatal: the app
/// keeps running, just without working hotkeys.
pub fn spawn(app: AppHandle, config: Config) {
    tauri::async_runtime::spawn(async move {
        if let Err(err) = run(app, config).await {
            log::error!("Wayland GlobalShortcuts portal error: {err}");
            log::error!(
                "Your desktop environment may not support the GlobalShortcuts portal \
                 (needs GNOME 42+ or KDE Plasma 6+); hotkeys will not work."
            );
        }
    });
}

fn non_empty(s: &str) -> Option<&str> {
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

struct SharedState {
    camera_held: Arc<AtomicBool>,
    mock_on: Arc<AtomicBool>,
    pinned: Arc<AtomicBool>,
}

async fn run(app: AppHandle, config: Config) -> ashpd::Result<()> {
    let portal = GlobalShortcuts::new().await?;
    let session: Session<GlobalShortcuts> =
        portal.create_session(Default::default()).await?;

    let mut shortcuts = vec![
        NewShortcut::new("camera", "Show camera").preferred_trigger(non_empty(&config.hotkey)),
        NewShortcut::new("mock", "Toggle mock notification")
            .preferred_trigger(non_empty(&config.mock_notification.hotkey)),
    ];
    if !config.zoom.key.is_empty() {
        shortcuts.push(
            NewShortcut::new("zoom", "Toggle zoom").preferred_trigger(Some(config.zoom.key.as_str())),
        );
    }
    if !config.pin_key.is_empty() {
        shortcuts.push(
            NewShortcut::new("pin", "Pin camera").preferred_trigger(Some(config.pin_key.as_str())),
        );
    }
    for action in PAN_ACTIONS {
        shortcuts.push(NewShortcut::new(action, format!("Camera {action}")));
    }

    portal
        .bind_shortcuts(&session, &shortcuts, None, Default::default())
        .await?
        .response()?;
    log::info!("Wayland GlobalShortcuts portal: shortcuts bound, waiting for activation");

    let state = SharedState {
        camera_held: Arc::new(AtomicBool::new(false)),
        mock_on: Arc::new(AtomicBool::new(false)),
        pinned: Arc::new(AtomicBool::new(false)),
    };
    // One "currently repeating" flag per pan direction, mirroring the
    // repeat-while-held behavior of the Windows/X11 backend.
    let pan_held: [(&'static str, Arc<AtomicBool>); 4] = [
        ("pan_up", Arc::new(AtomicBool::new(false))),
        ("pan_down", Arc::new(AtomicBool::new(false))),
        ("pan_left", Arc::new(AtomicBool::new(false))),
        ("pan_right", Arc::new(AtomicBool::new(false))),
    ];

    let mut activated = portal.receive_activated().await?;
    let mut deactivated = portal.receive_deactivated().await?;

    loop {
        tokio::select! {
            Some(ev) = activated.next() => {
                handle_activated(&app, ev.shortcut_id(), &state, &pan_held);
            }
            Some(ev) = deactivated.next() => {
                handle_deactivated(&app, ev.shortcut_id(), &state, &pan_held);
            }
            else => break,
        }
    }
    Ok(())
}

fn handle_activated(
    app: &AppHandle,
    id: &str,
    state: &SharedState,
    pan_held: &[(&'static str, Arc<AtomicBool>); 4],
) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    match id {
        "camera" => {
            state.pinned.store(false, Ordering::SeqCst);
            state.camera_held.store(true, Ordering::SeqCst);
            let _ = window.emit(MODE_EVENT, "camera");
            let _ = window.show();
        }
        "mock" => {
            let now_on = !state.mock_on.load(Ordering::SeqCst);
            state.mock_on.store(now_on, Ordering::SeqCst);
            if now_on {
                let _ = window.emit(MODE_EVENT, "mock");
                let _ = window.show();
            } else if state.camera_held.load(Ordering::SeqCst) || state.pinned.load(Ordering::SeqCst) {
                let _ = window.emit(MODE_EVENT, "camera");
                let _ = window.show();
            } else {
                let _ = window.hide();
            }
        }
        // Zoom, pin and pan are bound for the whole session (the portal has
        // no cheap way to bind/unbind on the fly), so we gate them in
        // software: they only do anything while the camera is actually
        // held, matching the Windows/X11 behavior.
        "zoom" => {
            if state.camera_held.load(Ordering::SeqCst) {
                let _ = window.emit(ZOOM_EVENT, "toggle");
            }
        }
        "pin" => {
            if state.camera_held.load(Ordering::SeqCst) {
                let now = !state.pinned.load(Ordering::SeqCst);
                state.pinned.store(now, Ordering::SeqCst);
            }
        }
        action if PAN_ACTIONS.contains(&action) => {
            if !state.camera_held.load(Ordering::SeqCst) {
                return;
            }
            let Some((_, held)) = pan_held.iter().find(|(a, _)| *a == action) else {
                return;
            };
            if held.swap(true, Ordering::SeqCst) {
                return;
            }
            let _ = window.emit(ZOOM_EVENT, action);
            let app2 = app.clone();
            let held2 = held.clone();
            let camera_held2 = state.camera_held.clone();
            let action = action.to_string();
            std::thread::spawn(move || loop {
                std::thread::sleep(Duration::from_millis(60));
                if !held2.load(Ordering::SeqCst) || !camera_held2.load(Ordering::SeqCst) {
                    break;
                }
                if let Some(w) = app2.get_webview_window(OVERLAY_LABEL) {
                    let _ = w.emit(ZOOM_EVENT, action.as_str());
                }
            });
        }
        _ => {}
    }
}

fn handle_deactivated(
    app: &AppHandle,
    id: &str,
    state: &SharedState,
    pan_held: &[(&'static str, Arc<AtomicBool>); 4],
) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };
    match id {
        "camera" => {
            state.camera_held.store(false, Ordering::SeqCst);
            if state.pinned.load(Ordering::SeqCst) {
                return;
            }
            let _ = window.emit(ZOOM_EVENT, "reset");
            if state.mock_on.load(Ordering::SeqCst) {
                let _ = window.emit(MODE_EVENT, "mock");
                let _ = window.show();
            } else {
                let _ = window.hide();
            }
        }
        action if PAN_ACTIONS.contains(&action) => {
            if let Some((_, held)) = pan_held.iter().find(|(a, _)| *a == action) {
                held.store(false, Ordering::SeqCst);
            }
        }
        // "mock", "zoom" and "pin" are toggles; release is a no-op for them.
        _ => {}
    }
}
