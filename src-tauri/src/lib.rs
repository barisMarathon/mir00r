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
const DEFAULT_HOTKEY: &str = "Ctrl+Shift+C";
const DEFAULT_MOCK_HOTKEY: &str = "Ctrl+Shift+N";
// Not: "ShiftRight" gibi sol/sag ayrimli tuslar kurulu global-hotkey
// surumunde Windows'ta desteklenmiyor (VK kod eslemesi yok), o yuzden
// varsayilan olarak sade bir tus kullaniyoruz. Config'ten degistirilebilir.
const DEFAULT_ZOOM_KEY: &str = "/";
const DEFAULT_PIN_KEY: &str = "T";
// WebView2'de ayni uygulama/user-data-folder icindeki TUM pencereler ayni
// ortam (environment) secenklerini paylasmak zorunda. Overlay penceresi bu
// bayraklarla olusturuldugu icin, sonradan olusturulan HER pencere (orn.
// ayarlar) da AYNI bayraklari kullanmali; aksi halde HRESULT 0x8007139F
// hatasiyla webview olusturma basarisiz oluyor.
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
            title: "Bildirim".to_string(),
            message: "Bu, onceden belirlenmis sahte bir bildirim mesajidir.".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ZoomConfig {
    /// Zoom'u ac/kapa yapan tek tus, orn. "/" ya da "F9". Modifiersiz tek tus.
    key: String,
    /// Zoom acikken video kac kat buyusun (orn. 2.0 = 2x).
    zoom_level: f64,
    /// Ok tuslarina her basista pan miktari (yuzde puani, video boyutuna gore).
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
    /// Kamera basiliyken bu tusa basmak, o anki zoom/pozisyonda kamerayi
    /// sabitler; Home birakilsa bile acik kalir. Tekrar basmak sabitlemeyi
    /// kaldirir. Modifiersiz tek tus.
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
    // Kayittan once kisayollarin gecerli oldugunu dogruluyoruz; aksi halde
    // bozuk bir config bir sonraki acilista uygulamanin cokmesine yol acar.
    config
        .hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("Gecersiz kamera kisayolu: {e}"))?;
    config
        .mock_notification
        .hotkey
        .parse::<Shortcut>()
        .map_err(|e| format!("Gecersiz bildirim kisayolu: {e}"))?;
    config
        .zoom
        .key
        .parse::<Shortcut>()
        .map_err(|e| format!("Gecersiz zoom tusu: {e}"))?;
    config
        .pin_key
        .parse::<Shortcut>()
        .map_err(|e| format!("Gecersiz pin tusu: {e}"))?;

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

/// Sahte bildirim tusu HOLD degil TOGGLE: bir basista acilir ve kalir,
/// tekrar basilinca kapanir (birakma olayi yok sayilir). Kamera basiliyken
/// (ya da sabitliyken) bildirime basilirsa bildirim onune gecer; bildirim
/// kapatilirsa ve kamera hala basili/sabitliyse kameraya geri donulur,
/// degilse pencere tamamen gizlenir.
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

/// Kamera kisayolu icin ozel handler: normal show/hide davranisina ek olarak,
/// kamera basili oldugu SURECE zoom tusunu ve 4 ok tusunu GLOBAL olarak
/// kaydedip, birakildiginda hemen geri kaldiriyor. Boylece ok tuslari sadece
/// kamera acikken sistem genelini etkiliyor, digerinde normal calisiyorlar.
/// Kamera kapandiginda frontend'e "reset" gonderilir ki bir sonraki acilista
/// zoom/pan varsayilana donsun.
///
/// on_shortcut/unregister cagrilari, global-shortcut eklentisinin bizi
/// cagirdigi ayni callback icinden (kendi kisayol-yonetimi kilidini
/// tutarken) yapilirsa kendi kendine kilitlenmeye (deadlock) yol aciyor.
/// run_on_main_thread ZATEN ana thread'deyken cagrilirsa closure'i hemen
/// (ayni cagri yigininda) calistirdigindan tek basina yetmiyor. Bu yuzden
/// isi once AYRI bir OS thread'ine atip, oradan run_on_main_thread
/// cagiriyoruz; boylece gercekten ana donguye ertelenmis oluyor ve mevcut
/// kilit devam etmeden once serbest kaliyor.
fn camera_hold_handler(
    zoom_shortcut: Shortcut,
    pan_shortcuts: [(&'static str, Shortcut); 4],
    pin_shortcut: Shortcut,
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
                        // Home hizlica birak-bas yapilirsa onceki birakisin kayit
                        // silme islemi henuz bitmemis olabilir; zaten kayitliysa
                        // tekrar denemeyip sessizce atla.
                        if !gs.is_registered(zoom_shortcut) {
                            if let Err(err) = gs.on_shortcut(zoom_shortcut, |app, _s, ev| {
                                if matches!(ev.state(), ShortcutState::Pressed) {
                                    if let Some(w) = app.get_webview_window(OVERLAY_LABEL) {
                                        let _ = w.emit(ZOOM_EVENT, "toggle");
                                    }
                                }
                            }) {
                                log::error!("zoom kisayolu kaydedilemedi: {err}");
                            }
                        }
                        // Sabitleme (pin) tusu da sadece kamera basiliyken
                        // aktif: basinca "pinned" bayragini ters ceviriyor.
                        if !gs.is_registered(pin_shortcut) {
                            let pinned_for_pin = pinned_outer.clone();
                            if let Err(err) = gs.on_shortcut(pin_shortcut, move |_app, _s, ev| {
                                if matches!(ev.state(), ShortcutState::Pressed) {
                                    let now = !pinned_for_pin.load(Ordering::SeqCst);
                                    pinned_for_pin.store(now, Ordering::SeqCst);
                                }
                            }) {
                                log::error!("pin kisayolu kaydedilemedi: {err}");
                            }
                        }
                        for (action, shortcut) in pan_shortcuts {
                            if gs.is_registered(shortcut) {
                                continue;
                            }
                            // Ok tusu SADECE bir kez "Pressed" olayi veriyor
                            // (OS tekrari yok); basili tutuldukca hareketin
                            // devam etmesi icin kendi tekrar dongumuzu
                            // baslatiyoruz, "Released" gelene ya da kamera
                            // birakilana kadar calisiyor.
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
                                log::error!("'{action}' kisayolu kaydedilemedi: {err}");
                            }
                        }
                    });
                    if let Err(err) = result {
                        log::error!("zoom/pan kisayollari ana thread'e gonderilemedi: {err}");
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
                        let _ = gs.unregister(zoom_shortcut);
                        let _ = gs.unregister(pin_shortcut);
                        for (_, shortcut) in pan_shortcuts {
                            let _ = gs.unregister(shortcut);
                        }
                    });
                    if let Err(err) = result {
                        log::error!("zoom/pan kisayollari kaldirilamadi: {err}");
                    }
                });
                // Sabitlenmisse (pin) pencereyi oldugu gibi (mevcut
                // zoom/pozisyonda) birakiyoruz; gizleme/sifirlama yok.
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
                .additional_browser_args(WEBVIEW_BROWSER_ARGS)
                // WebView2 yerel dosyalari diskte onbelleklediginden, gelistirme
                // sirasinda dist/ altindaki dosyalari degistirmek uygulamayi
                // yeniden baslatsak bile eski surumu gostermeye devam edebiliyor.
                // Her yaniti "no-cache" yaparak bunu engelliyoruz.
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
                format!("gecersiz hotkey '{}' config dosyasinda: {err}", config.hotkey)
            })?;
            let zoom_shortcut: Shortcut = config.zoom.key.parse().map_err(|err| {
                format!("gecersiz zoom.key '{}' config dosyasinda: {err}", config.zoom.key)
            })?;
            let pan_shortcuts: [(&'static str, Shortcut); 4] = [
                ("pan_up", "Up".parse().expect("gecerli sabit kisayol")),
                ("pan_down", "Down".parse().expect("gecerli sabit kisayol")),
                ("pan_left", "Left".parse().expect("gecerli sabit kisayol")),
                ("pan_right", "Right".parse().expect("gecerli sabit kisayol")),
            ];
            let pin_shortcut: Shortcut = config.pin_key.parse().map_err(|err| {
                format!("gecersiz pin_key '{}' config dosyasinda: {err}", config.pin_key)
            })?;
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
            log::info!("kamera kisayolu kaydedildi: {}", config.hotkey);
            log::info!(
                "zoom tusu: {} (buyutme: {}x, pan adimi: %{}), pin tusu: {}",
                config.zoom.key,
                config.zoom.zoom_level,
                config.zoom.pan_step,
                config.pin_key
            );

            let mock_shortcut: Shortcut = config.mock_notification.hotkey.parse().map_err(|err| {
                format!(
                    "gecersiz mock_notification.hotkey '{}' config dosyasinda: {err}",
                    config.mock_notification.hotkey
                )
            })?;
            app.global_shortcut().on_shortcut(
                mock_shortcut,
                mock_toggle_handler(camera_held, pinned, mock_on),
            )?;
            log::info!(
                "sahte bildirim kisayolu kaydedildi (toggle): {}",
                config.mock_notification.hotkey
            );

            let settings_item = MenuItem::with_id(app, "settings", "Ayarlar", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Cikis", true, None::<&str>)?;
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
                            // Pencere zaten varsa sadece goster/odakla.
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

                            // Kamera penceresi kisa sure once olusturulduysa
                            // WebView2 ortami henuz tam oturmamis olabiliyor
                            // ve ikinci pencere olusturma HRESULT 0x8007139F
                            // hatasiyla basarisiz oluyor. Kisa aralarla
                            // birkac kez tekrar deniyoruz.
                            for attempt in 1..=6 {
                                let (tx, rx) = std::sync::mpsc::channel();
                                let app2 = app.clone();
                                let sent = app.run_on_main_thread(move || {
                                    let result = WebviewWindowBuilder::new(
                                        &app2,
                                        SETTINGS_LABEL,
                                        WebviewUrl::App("settings.html".into()),
                                    )
                                    .title("Mir00r Ayarlar")
                                    .inner_size(480.0, 700.0)
                                    .resizable(true)
                                    .additional_browser_args(WEBVIEW_BROWSER_ARGS)
                                    .build()
                                    .map(|_| ())
                                    .map_err(|e| e.to_string());
                                    let _ = tx.send(result);
                                });
                                if sent.is_err() {
                                    log::error!("ayarlar penceresi ana thread'e gonderilemedi");
                                    break;
                                }
                                match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                                    Ok(Ok(())) => break,
                                    Ok(Err(err)) => {
                                        log::error!(
                                            "ayarlar penceresi olusturulamadi (deneme {attempt}): {err}"
                                        );
                                        std::thread::sleep(std::time::Duration::from_millis(
                                            400 * attempt,
                                        ));
                                    }
                                    Err(_) => {
                                        log::error!("ayarlar penceresi olusturma zaman asimina ugradi");
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
