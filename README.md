# Mir00r

See who's behind you without turning around — an instant, offline webcam peek triggered by a hotkey.

Open offices, headphones on, someone walks up behind you — and you don't want to spin around every five minutes just to check. Mir00r gives you a fast, silent way to look: hold a hotkey and a small camera preview pops up on your screen, showing you exactly what's behind you. Let go, and it vanishes. No recording, no accounts, no cloud — everything runs 100% offline on your own machine.

Built with [Tauri](https://tauri.app) (Rust + a lightweight webview), so it's small, fast, and native.

Feel free to fork it, open issues, or send a PR — more features (and more polish) are very welcome.

## Features

- **Camera (hold):** Hold the hotkey to show the camera, release to hide it.
  The camera stream is kept running continuously in the background, so it
  appears instantly with no restart delay.
- **Mock notification (toggle):** A separate hotkey opens a fake,
  Windows-notification-style card with a title/message you configure ahead
  of time; it stays open until you toggle it again. Pressing it while the
  camera is held brings the notification to the front; if it's still on
  when the camera hotkey is released, it stays visible.
- **Zoom + Pan:** While the camera is shown, a key (disabled by default —
  set one in config/Settings) toggles 2x zoom on/off; pressing an arrow key
  turns zoom on too if it's off and pans in that direction, and holding it
  keeps panning. These keys are only active system-wide while the camera
  hotkey is held, and are released the instant you let go.
- **Pin:** While the camera is held, pressing the pin key (default `T`)
  pins the camera at its current zoom/position — it stays open even after
  you release the camera hotkey. Tapping the camera hotkey again, or
  pressing the pin key again, removes the pin.
- **Settings GUI:** "Settings" in the tray menu opens a window where you
  can edit the whole config without touching the file by hand, including a
  drag-and-drop picker (on a small map of your monitors) for the camera's
  X/Y position.
- There's no visible main window; the tray icon has "Settings" and "Quit".

## How it works

- Hotkey press/release is detected via `tauri-plugin-global-shortcut`: the
  press is caught instantly through the OS's native `RegisterHotKey` API,
  and the release is detected by polling every 50ms (fast enough to be
  imperceptible).
- The camera and the mock notification are shown in the SAME
  window/webview; which one is visible is decided in JS via a
  `mir00r://mode` event. Using two separate WebView2 windows caused a race
  condition (one still initializing while the other is being created), so
  this approach was used instead; secondary windows like Settings only open
  reliably once the first window has fully settled and every window shares
  the same `additional_browser_args`.

## Configuration

Easiest way: right-click the Mir00r tray icon ➜ **Settings**.

To edit by hand, the config file lives at:

`%APPDATA%\com.mir00r.app\mir00r.config.json`

```json
{
  "hotkey": "Home",
  "region": { "x": 100, "y": 100, "width": 320, "height": 240 },
  "mock_notification": {
    "hotkey": "Pause",
    "title": "Notification",
    "message": "This is a pre-configured fake notification message."
  },
  "zoom": { "key": "", "zoom_level": 2.0, "pan_step": 8.0 },
  "pin_key": "T"
}
```

- `hotkey`: the camera hotkey (e.g. `"Ctrl+Alt+M"`, `"F9"`, `"Home"`). Any
  modifiers (Ctrl/Shift/Alt) must come first, with a single key at the end.
- `region`: the camera window's position (`x`, `y`) and size (`width`,
  `height`) on screen, in pixels.
- `mock_notification.hotkey`: the toggle hotkey for the mock notification.
- `zoom.key`: the zoom on/off key; **must be a single key with no
  modifiers**. Left empty by default (zoom/pan disabled) — side-specific
  keys like `ShiftRight` aren't supported on Windows by the installed
  global-hotkey version.
- `pin_key`: the key that pins the camera; same rule, a single key with no
  modifiers. Leave empty to disable.

You need to close and reopen the app after making changes (the Settings
GUI's automatic restart is currently disabled because it races with
shortcut registration under the `cargo tauri dev` wrapper).

## Development

No Node/npm needed; the frontend is plain HTML/CSS/JS (`dist/`), the
backend is Rust (`src-tauri/`).

```bash
cargo tauri dev
```

Build:

```bash
cargo tauri build
```

## Known limitations (for next steps)

- The camera stream stays open the whole time the app runs, so the camera
  LED stays lit too (a deliberate tradeoff for speed).
- There's no special handling yet to stop the window from stealing focus
  from whatever app you were using when it's shown.
- Saving in the Settings GUI writes to disk but doesn't restart the app
  automatically; you need to close and reopen it by hand.
