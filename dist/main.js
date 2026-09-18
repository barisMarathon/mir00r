const video = document.getElementById("cam");
const status = document.getElementById("status");
const frame = document.getElementById("frame");
const toast = document.getElementById("toast");
const toastTitle = document.getElementById("toast-title");
const toastMessage = document.getElementById("toast-message");

function showStatus(message) {
  status.textContent = message;
  status.classList.remove("hidden");
}

function hideStatus() {
  status.classList.add("hidden");
}

function logToBackend(message) {
  try {
    window.__TAURI__?.core?.invoke("log_frontend", { message });
  } catch (_) {
    // backend'e ulasilamiyorsa sessizce yut, UI'yi etkilemesin
  }
}

let mockContentLoaded = false;
async function ensureMockContent() {
  if (mockContentLoaded) return;
  try {
    const config = await window.__TAURI__.core.invoke("get_mock_notification");
    toastTitle.textContent = config.title;
    toastMessage.textContent = config.message;
    mockContentLoaded = true;
  } catch (err) {
    toastTitle.textContent = "Bildirim";
    toastMessage.textContent = "Icerik yuklenemedi.";
    logToBackend("mock bildirim icerik hatasi: " + err);
  }
}

function setMode(mode) {
  if (mode === "mock") {
    ensureMockContent();
    frame.classList.add("hidden");
    toast.classList.remove("hidden");
  } else {
    toast.classList.add("hidden");
    frame.classList.remove("hidden");
  }
}

// Backend hangi kisayolun basildigini "mir00r://mode" olayiyla bildiriyor
// (payload: "camera" | "mock"); ayni pencere/webview icinde sadece
// gorunen paneli degistiriyoruz, boylece ikinci bir WebView2 penceresi
// olusturma ihtiyaci ortadan kalkiyor.
window.__TAURI__?.event?.listen("mir00r://mode", (event) => {
  setMode(event.payload);
});

let zoomLevel = 1;
let panX = 0;
let panY = 0;
let zoomTarget = 2;
let panStep = 8;

window.__TAURI__?.core
  ?.invoke("get_zoom_config")
  .then((cfg) => {
    zoomTarget = cfg.zoom_level;
    panStep = cfg.pan_step;
  })
  .catch((err) => logToBackend("zoom config yuklenemedi: " + err));

function applyVideoTransform() {
  video.style.transform = `scaleX(-1) scale(${zoomLevel}) translate(${panX}%, ${panY}%)`;
}

function toggleZoom() {
  if (zoomLevel === 1) {
    zoomLevel = zoomTarget;
  } else {
    zoomLevel = 1;
    panX = 0;
    panY = 0;
  }
  applyVideoTransform();
}

function panBy(dx, dy) {
  if (zoomLevel <= 1) return;
  const limit = (1 - 1 / zoomLevel) * 50;
  panX = Math.max(-limit, Math.min(limit, panX + dx));
  panY = Math.max(-limit, Math.min(limit, panY + dy));
  applyVideoTransform();
}

function resetZoom() {
  zoomLevel = 1;
  panX = 0;
  panY = 0;
  applyVideoTransform();
}

applyVideoTransform();

// Kamera acikken Sag Shift ile zoom acilip kapaniyor, ok tuslariyla
// zoomlu gorunum icinde gezinilebiliyor. Kamera kisayolu birakildiginda
// backend "reset" gonderiyor, boylece bir sonraki acilista zoom/pan
// varsayilana donuyor.
window.__TAURI__?.event?.listen("mir00r://zoom-action", (event) => {
  switch (event.payload) {
    case "toggle":
      toggleZoom();
      break;
    case "reset":
      resetZoom();
      break;
    case "pan_up":
      panBy(0, -panStep);
      break;
    case "pan_down":
      panBy(0, panStep);
      break;
    case "pan_left":
      panBy(panStep, 0);
      break;
    case "pan_right":
      panBy(-panStep, 0);
      break;
  }
});

function sampleBrightness() {
  if (!video.videoWidth) {
    return;
  }
  const canvas = document.createElement("canvas");
  canvas.width = 16;
  canvas.height = 16;
  const ctx = canvas.getContext("2d");
  ctx.drawImage(video, 0, 0, 16, 16);
  const data = ctx.getImageData(0, 0, 16, 16).data;
  let sum = 0;
  for (let i = 0; i < data.length; i += 4) {
    sum += (data[i] + data[i + 1] + data[i + 2]) / 3;
  }
  const avg = sum / (data.length / 4);
  logToBackend(`brightness ornek: ${avg.toFixed(1)} / 255`);
}

// Kamera akisi pencere gizliyken de baslatilir ve surekli acik tutulur;
// boylece kisayola basildiginda sadece pencere gosterilir, kamera
// yeniden baslatilmaz ve gecikme olmaz.
async function startCamera() {
  logToBackend("kamera erisimi isteniyor...");
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      video: true,
      audio: false,
    });
    video.srcObject = stream;
    try {
      await video.play();
    } catch (playErr) {
      logToBackend("video.play() hatasi: " + playErr.message);
    }
    hideStatus();
    const track = stream.getVideoTracks()[0];
    logToBackend("kamera basladi: " + (track ? track.label : "bilinmeyen cihaz"));
    setInterval(sampleBrightness, 5000);
  } catch (err) {
    showStatus("Kamera acilamadi: " + err.message);
    logToBackend("kamera HATASI: " + err.name + " - " + err.message);
    setTimeout(startCamera, 2000);
  }
}

startCamera();
