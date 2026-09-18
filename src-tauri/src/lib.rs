use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

const OVERLAY_LABEL: &str = "overlay";
const MODE_EVENT: &str = "mir00r://mode";
const CONFIG_FILE_NAME: &str = "mir00r.config.json";
const DEFAULT_HOTKEY: &str = "Ctrl+Shift+C";
const DEFAULT_MOCK_HOTKEY: &str = "Ctrl+Shift+N";

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
            title: "Bildirim".to_string(),
            message: "Bu, onceden belirlenmis sahte bir bildirim mesajidir.".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Config {
    /// Accelerator string, e.g. "Ctrl+Shift+C". Modifiers must come before the key.
    hotkey: String,
    region: Region,
    #[serde(default)]
    mock_notification: MockNotificationConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkey: DEFAULT_HOTKEY.to_string(),
            region: Region::default(),
            mock_notification: MockNotificationConfig::default(),
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
            log::error!("config dizini bulunamadi: {err}");
            return Config::default();
        }
    };

    if let Ok(raw) = fs::read_to_string(&path) {
        match serde_json::from_str::<Config>(&raw) {
            Ok(config) => return config,
            Err(err) => log::error!("config.json okunamadi ({err}), varsayilanlar kullaniliyor"),
        }
    }

    let default_config = Config::default();
    if let Ok(serialized) = serde_json::to_string_pretty(&default_config) {
        if let Err(err) = fs::write(&path, serialized) {
            log::error!("varsayilan config yazilamadi: {err}");
        } else {
            log::info!("varsayilan config olusturuldu: {}", path.display());
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

/// Kisayol basiliyken tek overlay penceresini gosterir (once icerik modunu
/// "camera" ya da "mock" olarak bildirir), birakildiginda gizler. Iki ayri
/// WebView2 penceresi kullanmak (biri hala kurulurken digeri olusturulmaya
/// calisildiginda) HRESULT 0x8007139F hatasina yol actigi icin, kamera ve
/// sahte bildirim ayni pencere/webview icinde, JS tarafinda mod degistirilerek
/// gosteriliyor.
fn hold_to_toggle_mode(
    mode: &'static str,
) -> impl Fn(&AppHandle, &Shortcut, ShortcutEvent) + Send + Sync + 'static {
    move |app, _shortcut, event| {
        let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
            return;
        };
        match event.state() {
            ShortcutState::Pressed => {
                let _ = window.emit(MODE_EVENT, mode);
                let _ = window.show();
            }
            ShortcutState::Released => {
                let _ = window.hide();
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![log_frontend, get_mock_notification])
        .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info).build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let config = load_or_create_config(&handle);
            app.manage(config.mock_notification.clone());

            WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("index.html".into()))
                .title("Mir00r")
                .decorations(false)
                // Pencere transparent olunca WebView2'nin donanim hizlandirmali
                // video katmani dogru kompoze edilmiyor ve kamera goruntusu
                // neredeyse siyah/karanlik cikiyor. Opak pencere bunu cozuyor.
                .transparent(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .resizable(false)
                .shadow(false)
                .visible(false)
                .focused(false)
                // Kamera izni penceresi kucuk/odaksiz pencerede hic gorunup
                // tiklanamayabilir; Chromium'a izin dialogunu atlatip
                // varsayilan cihazi otomatik onaylatiyoruz. Ayrica frameless +
                // always-on-top pencerelerde donanim hizlandirmali video
                // decode bazen siyah/karanlik goruntu verdigi icin yazilim
                // decode'a zorluyoruz.
                .additional_browser_args(
                    "--use-fake-ui-for-media-stream --disable-gpu-compositing --disable-accelerated-video-decode",
                )
                .position(config.region.x as f64, config.region.y as f64)
                .inner_size(config.region.width as f64, config.region.height as f64)
                .build()?;

            let camera_shortcut: Shortcut = config.hotkey.parse().map_err(|err| {
                format!("gecersiz hotkey '{}' config dosyasinda: {err}", config.hotkey)
            })?;
            app.global_shortcut()
                .on_shortcut(camera_shortcut, hold_to_toggle_mode("camera"))?;
            log::info!("kamera kisayolu kaydedildi: {}", config.hotkey);

            let mock_shortcut: Shortcut = config.mock_notification.hotkey.parse().map_err(|err| {
                format!(
                    "gecersiz mock_notification.hotkey '{}' config dosyasinda: {err}",
                    config.mock_notification.hotkey
                )
            })?;
            app.global_shortcut()
                .on_shortcut(mock_shortcut, hold_to_toggle_mode("mock"))?;
            log::info!(
                "sahte bildirim kisayolu kaydedildi: {}",
                config.mock_notification.hotkey
            );

            let quit_item = MenuItem::with_id(app, "quit", "Cikis", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&quit_item])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&tray_menu)
                .tooltip("Mir00r")
                .on_menu_event(|app, event| {
                    if event.id() == "quit" {
                        app.exit(0);
                    }
                })
                .build(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
