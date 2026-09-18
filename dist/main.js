const video = document.getElementById("cam");
const status = document.getElementById("status");

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

function sampleBrightness() {
  if (!video.videoWidth) {
    logToBackend(
      `brightness ornek: video boyutu yok (readyState=${video.readyState}, paused=${video.paused})`,
    );
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

  const statusStyle = getComputedStyle(status);
  const topEl = document.elementFromPoint(
    Math.floor(window.innerWidth / 2),
    Math.floor(window.innerHeight / 2),
  );
  logToBackend(
    `brightness ornek: ${avg.toFixed(1)} / 255 | status(display=${statusStyle.display}, opacity=${statusStyle.opacity}, hiddenClass=${status.classList.contains("hidden")}, text="${status.textContent}") | ustteEleman=${topEl ? topEl.id || topEl.tagName : "yok"} | body.classList=${document.body.className} | bodyBg=${getComputedStyle(document.body).backgroundColor}`,
  );
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
    video.addEventListener("loadedmetadata", () => {
      logToBackend(
        `loadedmetadata: ${video.videoWidth}x${video.videoHeight}`,
      );
    });
    video.addEventListener("playing", () => {
      logToBackend("video playing event tetiklendi");
    });
    try {
      await video.play();
    } catch (playErr) {
      logToBackend("video.play() hatasi: " + playErr.message);
    }
    hideStatus();
    const track = stream.getVideoTracks()[0];
    logToBackend(
      "kamera basladi: " +
        (track ? track.label : "bilinmeyen cihaz") +
        " ayarlar=" +
        JSON.stringify(track ? track.getSettings() : {}),
    );
    setInterval(sampleBrightness, 2000);
  } catch (err) {
    showStatus("Kamera acilamadi: " + err.message);
    logToBackend("kamera HATASI: " + err.name + " - " + err.message);
    setTimeout(startCamera, 2000);
  }
}

startCamera();
