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
    // swallow silently if the backend is unreachable, don't affect the UI
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
    toastTitle.textContent = "Notification";
    toastMessage.textContent = "Could not load content.";
    logToBackend("mock notification content error: " + err);
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

// The backend announces which hotkey was pressed via the "mir00r://mode"
// event (payload: "camera" | "mock"); we just switch which panel is
// visible within the same window/webview, which avoids ever needing a
// second WebView2 window.
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
  .catch((err) => logToBackend("could not load zoom config: " + err));

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
  // Pressing an arrow key while zoom is off turns zoom on first, then pans
  // in that direction.
  if (zoomLevel === 1) {
    zoomLevel = zoomTarget;
  }
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

// While the camera is shown, the zoom key toggles zoom on/off and the arrow
// keys pan within the zoomed view. When the camera hotkey is released, the
// backend sends "reset" so zoom/pan return to their defaults the next time
// the camera is shown.
window.__TAURI__?.event?.listen("mir00r://zoom-action", (event) => {
  switch (event.payload) {
    case "toggle":
      toggleZoom();
      break;
    case "reset":
      resetZoom();
      break;
    case "pan_up":
      panBy(0, panStep);
      break;
    case "pan_down":
      panBy(0, -panStep);
      break;
    case "pan_left":
      panBy(-panStep, 0);
      break;
    case "pan_right":
      panBy(panStep, 0);
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
  logToBackend(`brightness sample: ${avg.toFixed(1)} / 255`);
}

// The camera stream is started even while the window is hidden and kept
// running continuously; that way the hotkey only needs to show the window,
// the camera is never restarted, and there's no delay.
async function startCamera() {
  logToBackend("requesting camera access...");
  try {
    const stream = await navigator.mediaDevices.getUserMedia({
      video: true,
      audio: false,
    });
    video.srcObject = stream;
    try {
      await video.play();
    } catch (playErr) {
      logToBackend("video.play() error: " + playErr.message);
    }
    hideStatus();
    const track = stream.getVideoTracks()[0];
    logToBackend("camera started: " + (track ? track.label : "unknown device"));
    setInterval(sampleBrightness, 5000);
  } catch (err) {
    showStatus("Could not start camera: " + err.message);
    logToBackend("camera ERROR: " + err.name + " - " + err.message);
    setTimeout(startCamera, 2000);
  }
}

startCamera();
