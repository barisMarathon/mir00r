use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

const OVERLAY_LABEL: &str = "overlay";
const SETTINGS_LABEL: &str = "settings";
const MODE_EVENT: &str = "mir00r://mode";
const ZOOM_EVENT: &str = "mir00r://zoom-action";
const CONFIG_FILE_NAME: &str = "mir00r.config.json";
// Ctrl+Shift+C collides with the "copy" shortcut in most Linux terminals, so
// we default to plain, rarely-used keys instead.
const DEFAULT_HOTKEY: &str = "Home";
const DEFAULT_MOCK_HOTKEY: &str = "Pause";
// Left empty by default so the feature is opt-in; the user can set their own
// key in config or the Settings GUI. Note: side-specific keys like
// "ShiftRight" aren't supported on Windows by the installed global-hotkey
// version (no VK code mapping for them).
const DEFAULT_ZOOM_KEY: &str = "";
const DEFAULT_PIN_KEY: &str = "T";
// All windows created within the same app / user-data-folder in WebView2
// must share the same environment options. The overlay window is created
// with these flags, so every window created afterwards (e.g. settings) must
// use the SAME flags, otherwise webview creation fails with HRESULT
// 0x8007139F.
const WEBVIEW_BROWSER_ARGS: &str =
    "--use-fake-ui-for-media-stream --disable-gpu-compositing --disable-accelerated-video-decode";

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Region {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl Default for Region {
    fn default() -> Self {
        Self {
            x: 100,
            y: 100,
            width: 320,
            height: 240,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct MockNotificationConfig {
    /// Accelerator string, e.g. "Ctrl+Shift+N". Modifiers must come before the key.
    hotkey: String,
    title: String,
    message: String,
}

impl Default for MockNotificationConfig {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_MOCK_HOTKEY.to_string(),
            title: "Notification".to_string(),
            message: "This is a pre-configured fake notification message.".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ZoomConfig {
    /// Single key that toggles zoom on/off, e.g. "/" or "F9". No modifiers.
    key: String,
    /// How much the video should zoom in while active (e.g. 2.0 = 2x).
    zoom_level: f64,
    /// Pan amount per arrow-key press (percentage points, relative to the video size).
    pan_step: f64,
}

impl Default for ZoomConfig {
    fn default() -> Self {
        Self {
            key: DEFAULT_ZOOM_KEY.to_string(),
            zoom_level: 2.0,
            pan_step: 8.0,
        }
    }
}

fn default_pin_key() -> String {
    DEFAULT_PIN_KEY.to_string()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    /// Accelerator string, e.g. "Ctrl+Shift+C". Modifiers must come before the key.
    hotkey: String,
    region: Region,
    #[serde(default)]
    mock_notification: MockNotificationConfig,
    #[serde(default)]
    zoom: ZoomConfig,
    /// Pressing this key while the camera hotkey is held pins the camera at
    /// its current zoom/position; it stays visible even after the camera
    /// hotkey is released. Pressing it again removes the pin. No modifiers.
    #[serde(default = "default_pin_key")]
    pin_key: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            region: Region::default(),
            mock_notification: MockNotificationConfig::default(),
            zoom: ZoomConfig::default(),
            pin_key: default_pin_key(),
        }
    }
}

fn config_path(app: &tauri::AppHandle) -> tauri::Result<PathBuf> {
    let dir = app.path().app_config_dir()?;
    fs::create_dir_all(&dir)?;
    Ok(dir.join(CONFIG_FILE_NAME))
}

fn load_or_create_config(app: &tauri::AppHandle) -> Config {
    let path = match config_path(app) {
        Ok(p) => p,
        Err(err) => {
            log::error!("could not resolve config directory: {err}");
            return Config::default();
        }
    };

    if let Ok(raw) = fs::read_to_string(&path) {
        match serde_json::from_str::<Config>(&raw) {
            Ok(config) => return config,
            Err(err) => log::error!("could not read config.json ({err}), using defaults"),
        }
    }

    let default_config = Config::default();
    if let Ok(serialized) = serde_json::to_string_pretty(&default_config) {
        if let Err(err) = fs::write(&path, serialized) {
            log::error!("could not write default config: {err}");
        } else {
            log::info!("created default config at: {}", path.display());
        }
    }
    default_config
}

#[tauri::command]
fn log_frontend(message: String) {
    log::info!("[frontend] {message}");
}

#[tauri::command]
fn get_mock_notification(state: tauri::State<MockNotificationConfig>) -> MockNotificationConfig {
    state.inner().clone()
}

#[tauri::command]
fn get_zoom_config(state: tauri::State<ZoomConfig>) -> ZoomConfig {
    state.inner().clone()
}

#[tauri::command]
fn get_config(app: AppHandle) -> Config {
    load_or_create_config(&app)
}

#[tauri::command]
fn save_config(app: AppHandle, config: Config) -> Result<(), String> {
    // Validate every hotkey before saving; a broken config would otherwise
    // crash the app on its next launch.
    config
        .hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("Invalid camera hotkey: {e}"))?;
    config
        .mock_notification
        .hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("Invalid notification hotkey: {e}"))?;
    // Empty zoom/pin keys are valid: they mean the feature is disabled.
    if !config.zoom.key.is_empty() {
        config
            .zoom
            .key
            .parse::<Shortcut>()
            .map_err(|e| format!("Invalid zoom key: {e}"))?;
    }
    if !config.pin_key.is_empty() {
        config
            .pin_key
            .parse::<Shortcut>()
            .map_err(|e| format!("Invalid pin key: {e}"))?;
    }

    let path = config_path(&app).map_err(|e| e.to_string())?;
    let serialized = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&path, serialized).map_err(|e| e.to_string())?;
    Ok(())
}

#[derive(Serialize)]
struct MonitorInfo {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

#[tauri::command]
fn get_monitors(window: tauri::WebviewWindow) -> Result<Vec<MonitorInfo>, String> {
    let monitors = window.available_monitors().map_err(|e| e.to_string())?;
    Ok(monitors
        .iter()
        .map(|m| MonitorInfo {
            x: m.position().x,
            y: m.position().y,
            width: m.size().width,
            height: m.size().height,
        })
        .collect())
}

#[tauri::command]
fn restart_app(app: AppHandle) {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
    app.exit(0);
}

/// The mock notification key is a TOGGLE, not a hold: one press opens it and
/// it stays open, another press closes it (release events are ignored).
/// If the camera is held (or pinned) when the notification is toggled on,
/// the notification takes over the view; closing the notification falls
/// back to the camera view if it's still held/pinned, otherwise the window
/// is hidden entirely.
fn mock_toggle_handler(
    camera_held: Arc<AtomicBool>,
    pinned: Arc<AtomicBool>,
    mock_on: Arc<AtomicBool>,
) -> impl Fn(&AppHandle, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    move |app, _shortcut, event| {
        if !matches!(event.state(), ShortcutState::Pressed) {
            return;
        }
        let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
            return;
        };
        let now_on = !mock_on.load(Ordering::SeqCst);
        mock_on.store(now_on, Ordering::SeqCst);
        if now_on {
            let _ = window.emit(MODE_EVENT, "mock");
            let _ = window.show();
        } else if camera_held.load(Ordering::SeqCst) || pinned.load(Ordering::SeqCst) {
            let _ = window.emit(MODE_EVENT, "camera");
            let _ = window.show();
        } else {
            let _ = window.hide();
        }
    }
}

/// Dedicated handler for the camera hotkey. On top of the normal show/hide
/// behavior, it registers the zoom key and the 4 arrow keys GLOBALLY for as
/// long as the camera hotkey is held, and removes them immediately on
/// release. This way those keys only affect the whole system while the
/// camera is actually shown; otherwise they behave normally everywhere
/// else. On release, the frontend gets a "reset" event so zoom/pan return
/// to their defaults the next time the camera is shown.
///
/// Calling on_shortcut/unregister from within the very callback the
/// global-shortcut plugin invokes us on (while it's holding its own
/// shortcut-management lock) causes a self-deadlock. run_on_main_thread
/// alone isn't enough either, because when it's called while already on the
/// main thread it just runs the closure immediately, in the same call
/// stack. So we first hand the work off to a SEPARATE OS thread, and call
/// run_on_main_thread from there; that way it's genuinely deferred to a
/// later turn of the main loop, after the current lock has actually been
/// released.
fn camera_hold_handler(
    zoom_shortcut: Option<Shortcut>,
    pan_shortcuts: [(&'static str, Shortcut); 4],
    pin_shortcut: Option<Shortcut>,
    camera_held: Arc<AtomicBool>,
    mock_on: Arc<AtomicBool>,
    pinned: Arc<AtomicBool>,
) -> impl Fn(&AppHandle, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    move |app, _shortcut, event| {
        let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
            return;
        };
        match event.state() {
            ShortcutState::Pressed => {
                // Every fresh press of the camera hotkey clears any active
                // pin first. So while pinned, a quick tap of the camera
                // hotkey (press then release) closes the view; holding it
                // down keeps showing the camera as usual, and it can be
                // re-pinned from there.
                pinned.store(false, Ordering::SeqCst);
                camera_held.store(true, Ordering::SeqCst);
                let _ = window.emit(MODE_EVENT, "camera");
                let _ = window.show();

                let app_outer = app.clone();
                let camera_held_outer = camera_held.clone();
                let pinned_outer = pinned.clone();
                std::thread::spawn(move || {
                    let app_inner = app_outer.clone();
                    let result = app_outer.run_on_main_thread(move || {
                        let gs = app_inner.global_shortcut();
                        // If the camera hotkey is pressed/released in quick
                        // succession, the previous release's unregister may
                        // not have finished yet; skip re-registering if it's
                        // already registered. Both keys are optional (empty
                        // config = feature disabled).
                        if let Some(zoom_shortcut) = zoom_shortcut {
                            if !gs.is_registered(zoom_shortcut) {
                                if let Err(err) = gs.on_shortcut(zoom_shortcut, |app, _s, ev| {
                                    if matches!(ev.state(), ShortcutState::Pressed) {
                                        if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
                                            let _ = w.emit(ZOOM_EVENT, "toggle");
                                        }
                                    }
                                }) {
                                    log::error!("could not register zoom shortcut: {err}");
                                }
                            }
                        }
                        // The pin key is also only active while the camera
                        // is held: pressing it flips the "pinned" flag.
                        if let Some(pin_shortcut) = pin_shortcut {
                            if !gs.is_registered(pin_shortcut) {
                                let pinned_for_pin = pinned_outer.clone();
                                if let Err(err) = gs.on_shortcut(pin_shortcut, move |_app, _s, ev| {
                                    if matches!(ev.state(), ShortcutState::Pressed) {
                                        let now = !pinned_for_pin.load(Ordering::SeqCst);
                                        pinned_for_pin.store(now, Ordering::SeqCst);
                                    }
                                }) {
                                    log::error!("could not register pin shortcut: {err}");
                                }
                            }
                        }
                        for (action, shortcut) in pan_shortcuts {
                            if gs.is_registered(shortcut) {
                                continue;
                            }
                            // An arrow key only ever fires a single
                            // "Pressed" event (no OS auto-repeat); to keep
                            // panning while it's held down we run our own
                            // repeat loop, which stops once "Released"
                            // arrives or the camera hotkey is released.
                            let key_held = Arc::new(AtomicBool::new(false));
                            let camera_held_for_pan = camera_held_outer.clone();
                            if let Err(err) = gs.on_shortcut(shortcut, move |app, _s, ev| {
                                match ev.state() {
                                    ShortcutState::Pressed => {
                                        if key_held.swap(true, Ordering::SeqCst) {
                                            return;
                                        }
                                        if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
                                            let _ = w.emit(ZOOM_EVENT, action);
                                        }
                                        let app2 = app.clone();
                                        let key_held2 = key_held.clone();
                                        let camera_held2 = camera_held_for_pan.clone();
                                        std::thread::spawn(move || loop {
                                            std::thread::sleep(std::time::Duration::from_millis(60));
                                            if !key_held2.load(Ordering::SeqCst)
                                                || !camera_held2.load(Ordering::SeqCst)
                                            {
                                                break;
                                            }
                                            if let Some(w) = app2.get_webview_window(OVERLAY_LABEL) {
                                                let _ = w.emit(ZOOM_EVENT, action);
                                            }
                                        });
                                    }
                                    ShortcutState::Released => {
                                        key_held.store(false, Ordering::SeqCst);
                                    }
                                }
                            }) {
                                log::error!("could not register '{action}' shortcut: {err}");
                            }
                        }
                    });
                    if let Err(err) = result {
                        log::error!("could not hand zoom/pan shortcuts to the main thread: {err}");
                    }
                });
            }
            ShortcutState::Released => {
                camera_held.store(false, Ordering::SeqCst);
                let app_outer = app.clone();
                std::thread::spawn(move || {
                    let app_inner = app_outer.clone();
                    let result = app_outer.run_on_main_thread(move || {
                        let gs = app_inner.global_shortcut();
                        if let Some(zoom_shortcut) = zoom_shortcut {
                            let _ = gs.unregister(zoom_shortcut);
                        }
                        if let Some(pin_shortcut) = pin_shortcut {
                            let _ = gs.unregister(pin_shortcut);
                        }
                        for (_, shortcut) in pan_shortcuts {
                            let _ = gs.unregister(shortcut);
                        }
                    });
                    if let Err(err) = result {
                        log::error!("could not remove zoom/pan shortcuts: {err}");
                    }
                });
                // If pinned, leave the window exactly as it is (current
                // zoom/position); no hiding or resetting.
                if pinned.load(Ordering::SeqCst) {
                    return;
                }
                let _ = window.emit(ZOOM_EVENT, "reset");
                if mock_on.load(Ordering::SeqCst) {
                    let _ = window.emit(MODE_EVENT, "mock");
                    let _ = window.show();
                } else {
                    let _ = window.hide();
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            log_frontend,
            get_mock_notification,
            get_zoom_config,
            get_config,
            save_config,
            get_monitors,
            restart_app
        ])
        .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let config = load_or_create_config(&handle);
            app.manage(config.mock_notification.clone());
            app.manage(config.zoom.clone());

            WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("index.html".into()))
                .title("Mir00r")
                .decorations(false)
                // With the window set to transparent, WebView2's
                // hardware-accelerated video layer doesn't compose
                // correctly and the camera image comes out nearly
                // black/dark. An opaque window fixes it.
                .transparent(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .visible(false)
                .focused(false)
                // The camera permission prompt may never be visible/clickable
                // on such a small, unfocused window; skip the dialog and
                // auto-approve the default device via Chromium's flag.
                // Also, frameless + always-on-top windows sometimes render
                // hardware-accelerated video as black/dark, so we force
                // software decode too.
                .additional_browser_args(WEBVIEW_BROWSER_ARGS)
                // WebView2 caches local files on disk, so editing files
                // under dist/ during development can keep showing the old
                // version even after restarting the app. Marking every
                // response "no-cache" prevents that.
                .on_web_resource_request(|_request, response| {
                    response.headers_mut().insert(
                        tauri::http::header::CACHE_CONTROL,
                        tauri::http::HeaderValue::from_static("no-cache, no-store, must-revalidate"),
                    );
                })
                .position(config.region.x as f64, config.region.y as f64)
                .inner_size(config.region.width as f64, config.region.height as f64)
                .build()?;

            let camera_shortcut: Shortcut = config.hotkey.parse().map_err(|err| {
                format!("invalid hotkey '{}' in config: {err}", config.hotkey)
            })?;
            // Zoom and pin are opt-in: an empty key means the feature is
            // disabled rather than a parse error.
            let zoom_shortcut: Option<Shortcut> = if config.zoom.key.is_empty() {
                None
            } else {
                Some(config.zoom.key.parse().map_err(|err| {
                    format!("invalid zoom.key '{}' in config: {err}", config.zoom.key)
                })?)
            };
            let pan_shortcuts: [(&'static str, Shortcut); 4] = [
                ("pan_up", "Up".parse().expect("valid built-in shortcut")),
                ("pan_down", "Down".parse().expect("valid built-in shortcut")),
                ("pan_left", "Left".parse().expect("valid built-in shortcut")),
                ("pan_right", "Right".parse().expect("valid built-in shortcut")),
            ];
            let pin_shortcut: Option<Shortcut> = if config.pin_key.is_empty() {
                None
            } else {
                Some(config.pin_key.parse().map_err(|err| {
                    format!("invalid pin_key '{}' in config: {err}", config.pin_key)
                })?)
            };
            let camera_held = Arc::new(AtomicBool::new(false));
            let mock_on = Arc::new(AtomicBool::new(false));
            let pinned = Arc::new(AtomicBool::new(false));

            app.global_shortcut().on_shortcut(
                camera_shortcut,
                camera_hold_handler(
                    zoom_shortcut,
                    pan_shortcuts,
                    pin_shortcut,
                    camera_held.clone(),
                    mock_on.clone(),
                    pinned.clone(),
                ),
            )?;
            log::info!("registered camera hotkey: {}", config.hotkey);
            let zoom_key_desc = if config.zoom.key.is_empty() {
                "disabled".to_string()
            } else {
                format!(
                    "{} (level: {}x, pan step: {}%)",
                    config.zoom.key, config.zoom.zoom_level, config.zoom.pan_step
                )
            };
            let pin_key_desc = if config.pin_key.is_empty() {
                "disabled".to_string()
            } else {
                config.pin_key.clone()
            };
            log::info!("zoom key: {zoom_key_desc}, pin key: {pin_key_desc}");

            let mock_shortcut: Shortcut = config.mock_notification.hotkey.parse().map_err(|err| {
                format!(
                    "invalid mock_notification.hotkey '{}' in config: {err}",
                    config.mock_notification.hotkey
                )
            })?;
            app.global_shortcut().on_shortcut(
                mock_shortcut,
                mock_toggle_handler(camera_held, pinned, mock_on),
            )?;
            log::info!(
                "registered mock notification hotkey (toggle): {}",
                config.mock_notification.hotkey
            );

            let settings_item = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&settings_item, &quit_item])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .tooltip("Mir00r")
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                        return;
                    }
                    if event.id() == "settings" {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            // If the window already exists, just show/focus it.
                            if app.get_webview_window(SETTINGS_LABEL).is_some() {
                                let app2 = app.clone();
                                let _ = app.run_on_main_thread(move || {
                                    if let Some(w) = app2.get_webview_window(SETTINGS_LABEL) {
                                        let _ = w.show();
                                        let _ = w.set_focus();
                                    }
                                });
                                return;
                            }

                            // If the camera window was created moments ago,
                            // the WebView2 environment may not have settled
                            // yet, and creating a second window fails with
                            // HRESULT 0x8007139F. Retry a few times with a
                            // short delay.
                            for attempt in 1..=6 {
                                let (tx, rx) = std::sync::mpsc::channel();
                                let app2 = app.clone();
                                let sent = app.run_on_main_thread(move || {
                                    let result = WebviewWindowBuilder::new(
                                        &app2,
                                        SETTINGS_LABEL,
                                        WebviewUrl::App("settings.html".into()),
                                    )
                                    .title("Mir00r Settings")
                                    .inner_size(480.0, 700.0)
                                    .resizable(true)
                                    .additional_browser_args(WEBVIEW_BROWSER_ARGS)
                                    .build()
                                    .map(|_| ())
                                    .map_err(|e| e.to_string());
                                    let _ = tx.send(result);
                                });
                                if sent.is_err() {
                                    log::error!("could not hand settings window creation to the main thread");
                                    break;
                                }
                                match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                                    Ok(Ok(())) => break,
                                    Ok(Err(err)) => {
                                        log::error!(
                                            "could not create settings window (attempt {attempt}): {err}"
                                        );
                                        std::thread::sleep(std::time::Duration::from_millis(
                                            400 * attempt,
                                        ));
                                    }
                                    Err(_) => {
                                        log::error!("settings window creation timed out");
                                        break;
                                    }
                                }
                            }
                        });
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
