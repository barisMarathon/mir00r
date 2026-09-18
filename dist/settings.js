const invoke = (cmd, args) => window.__TAURI__.core.invoke(cmd, args);

const el = {
  camHotkey: document.getElementById("cam-hotkey"),
  camWidth: document.getElementById("cam-width"),
  camHeight: document.getElementById("cam-height"),
  camX: document.getElementById("cam-x"),
  camY: document.getElementById("cam-y"),
  pinKey: document.getElementById("pin-key"),
  mockHotkey: document.getElementById("mock-hotkey"),
  mockTitle: document.getElementById("mock-title"),
  mockMessage: document.getElementById("mock-message"),
  zoomKey: document.getElementById("zoom-key"),
  zoomLevel: document.getElementById("zoom-level"),
  zoomPanStep: document.getElementById("zoom-pan-step"),
  preview: document.getElementById("monitor-preview"),
  bubble: document.getElementById("cam-bubble"),
  statusMsg: document.getElementById("status-msg"),
  saveBtn: document.getElementById("save-btn"),
};

let monitors = [];
let bounds = { minX: 0, minY: 0, width: 1920, height: 1080 };
let scale = 1;

function computeBounds(list) {
  const minX = Math.min(...list.map((m) => m.x));
  const minY = Math.min(...list.map((m) => m.y));
  const maxX = Math.max(...list.map((m) => m.x + m.width));
  const maxY = Math.max(...list.map((m) => m.y + m.height));
  return { minX, minY, width: maxX - minX, height: maxY - minY };
}

function renderMonitors() {
  el.preview.querySelectorAll(".monitor-rect").forEach((n) => n.remove());
  const previewRect = el.preview.getBoundingClientRect();
  scale = Math.min(previewRect.width / bounds.width, previewRect.height / bounds.height) * 0.92;
  const offsetX = (previewRect.width - bounds.width * scale) / 2;
  const offsetY = (previewRect.height - bounds.height * scale) / 2;

  monitors.forEach((m) => {
    const div = document.createElement("div");
    div.className = "monitor-rect";
    div.style.left = offsetX + (m.x - bounds.minX) * scale + "px";
    div.style.top = offsetY + (m.y - bounds.minY) * scale + "px";
    div.style.width = m.width * scale + "px";
    div.style.height = m.height * scale + "px";
    el.preview.insertBefore(div, el.bubble);
  });

  el.preview.dataset.offsetX = offsetX;
  el.preview.dataset.offsetY = offsetY;
}

function renderBubble() {
  const offsetX = parseFloat(el.preview.dataset.offsetX || "0");
  const offsetY = parseFloat(el.preview.dataset.offsetY || "0");
  const x = Number(el.camX.value);
  const y = Number(el.camY.value);
  const w = Number(el.camWidth.value);
  const h = Number(el.camHeight.value);
  el.bubble.style.left = offsetX + (x - bounds.minX) * scale + "px";
  el.bubble.style.top = offsetY + (y - bounds.minY) * scale + "px";
  el.bubble.style.width = Math.max(6, w * scale) + "px";
  el.bubble.style.height = Math.max(6, h * scale) + "px";
}

function screenXYFromBubblePx(leftPx, topPx) {
  const offsetX = parseFloat(el.preview.dataset.offsetX || "0");
  const offsetY = parseFloat(el.preview.dataset.offsetY || "0");
  const x = Math.round((leftPx - offsetX) / scale + bounds.minX);
  const y = Math.round((topPx - offsetY) / scale + bounds.minY);
  return { x, y };
}

function setupDrag() {
  let dragging = false;
  let startMouse = { x: 0, y: 0 };
  let startBubble = { left: 0, top: 0 };

  el.bubble.addEventListener("mousedown", (e) => {
    dragging = true;
    startMouse = { x: e.clientX, y: e.clientY };
    startBubble = { left: el.bubble.offsetLeft, top: el.bubble.offsetTop };
    e.preventDefault();
  });

  window.addEventListener("mousemove", (e) => {
    if (!dragging) return;
    const previewRect = el.preview.getBoundingClientRect();
    let newLeft = startBubble.left + (e.clientX - startMouse.x);
    let newTop = startBubble.top + (e.clientY - startMouse.y);
    newLeft = Math.max(0, Math.min(previewRect.width - el.bubble.offsetWidth, newLeft));
    newTop = Math.max(0, Math.min(previewRect.height - el.bubble.offsetHeight, newTop));
    el.bubble.style.left = newLeft + "px";
    el.bubble.style.top = newTop + "px";
    const { x, y } = screenXYFromBubblePx(newLeft, newTop);
    el.camX.value = x;
    el.camY.value = y;
  });

  window.addEventListener("mouseup", () => {
    dragging = false;
  });
}

function showStatus(message, kind) {
  el.statusMsg.textContent = message;
  el.statusMsg.className = kind || "";
}

async function load() {
  try {
    const [config, monitorList] = await Promise.all([
      invoke("get_config"),
      invoke("get_monitors"),
    ]);
    monitors = monitorList;
    bounds = computeBounds(monitors);

    el.camHotkey.value = config.hotkey;
    el.camWidth.value = config.region.width;
    el.camHeight.value = config.region.height;
    el.camX.value = config.region.x;
    el.camY.value = config.region.y;
    el.pinKey.value = config.pin_key;

    el.mockHotkey.value = config.mock_notification.hotkey;
    el.mockTitle.value = config.mock_notification.title;
    el.mockMessage.value = config.mock_notification.message;

    el.zoomKey.value = config.zoom.key;
    el.zoomLevel.value = config.zoom.zoom_level;
    el.zoomPanStep.value = config.zoom.pan_step;

    renderMonitors();
    renderBubble();
  } catch (err) {
    showStatus("Ayarlar yuklenemedi: " + err, "error");
  }
}

[el.camWidth, el.camHeight, el.camX, el.camY].forEach((input) => {
  input.addEventListener("input", renderBubble);
});
window.addEventListener("resize", () => {
  renderMonitors();
  renderBubble();
});

el.saveBtn.addEventListener("click", async () => {
  const config = {
    hotkey: el.camHotkey.value.trim(),
    region: {
      x: Number(el.camX.value),
      y: Number(el.camY.value),
      width: Number(el.camWidth.value),
      height: Number(el.camHeight.value),
    },
    pin_key: el.pinKey.value.trim(),
    mock_notification: {
      hotkey: el.mockHotkey.value.trim(),
      title: el.mockTitle.value,
      message: el.mockMessage.value,
    },
    zoom: {
      key: el.zoomKey.value.trim(),
      zoom_level: Number(el.zoomLevel.value),
      pan_step: Number(el.zoomPanStep.value),
    },
  };

  el.saveBtn.disabled = true;
  try {
    await invoke("save_config", { config });
    // Otomatik yeniden baslatma (restart_app), eski surec tam kapanmadan
    // yenisi ayni kisayollari kaydetmeye calisip cakisabiliyor (ozellikle
    // `cargo tauri dev` sarmalayicisinda). Su an icin kullaniciya manuel
    // yeniden baslatmasini soyluyoruz; exe'ye gecince tekrar ele alinacak.
    showStatus("Kaydedildi. Degisikliklerin gecmesi icin Mir00r'u kapatip tekrar ac.", "ok");
    el.saveBtn.disabled = false;
  } catch (err) {
    showStatus(String(err), "error");
    el.saveBtn.disabled = false;
  }
});

setupDrag();
load();
