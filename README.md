# Mir00r

Bir kisayol tusuna basili tuttugun surece ekranin belirledigin bir bolgesinde
kamera goruntusu acan, biraktiginda kapatan bir masaustu uygulamasi.

## Nasil calisiyor

- Uygulama acildiginda kamera akisi (`getUserMedia`) hemen baslatilir ve
  arka planda surekli acik tutulur. Kisayola basildiginda sadece onceden
  hazir olan pencere gosterilir; kamera yeniden baslatilmadigi icin gecikme
  olmaz. Kisayol birakildiginda pencere gizlenir.
- Kisayolun basilma/birakilma anlarini `tauri-plugin-global-shortcut`
  yakaliyor: basma isletim sisteminin native `RegisterHotKey` API'siyle
  aninda algilanir, birakma ise 50ms araliklarla yapilan bir kontrolle tespit
  edilir (goze gorunmeyecek kadar hizli).
- Uygulamanin gorunur bir ana penceresi yok; sistem tepsisinde (tray) bir
  simge ve "Cikis" secenegi bulunur.

## Ayarlar

Ilk calistirmada su konumda bir `mir00r.config.json` dosyasi olusturulur:

`%APPDATA%\com.mir00r.app\mir00r.config.json`

```json
{
  "hotkey": "Ctrl+Shift+C",
  "region": { "x": 100, "y": 100, "width": 320, "height": 240 }
}
```

- `hotkey`: degistirilebilir kisayol (orn. `"Ctrl+Alt+M"`, `"F9"`). Once
  varsa degistiriciler (Ctrl/Shift/Alt), en sonda tek bir tus olmali.
- `region`: kamera penceresinin ekrandaki konumu (`x`, `y`) ve boyutu
  (`width`, `height`), piksel cinsinden.

Degisiklikten sonra uygulamayi yeniden baslatmak gerekiyor (MVP asamasinda
canli yeniden yukleme yok).

## Gelistirme

Node/npm gerekmiyor; frontend duz HTML/CSS/JS (`dist/`), backend Rust
(`src-tauri/`).

```bash
cargo tauri dev
```

Derleme:

```bash
cargo tauri build
```

## Bilinen sinirlamalar (sonraki adimlar icin)

- Kamera akisi uygulama acikken surekli acik oldugu icin kamera LED'i de
  surekli yanik kalir (hiz icin bilincli tercih). Istenirse ileride
  "bekleme modunda kapat, ilk basista ac" secenegi eklenebilir.
- Pencere gosterilirken odak (focus) calan bir uygulamadan calinmasini
  engelleyen ozel bir davranis henuz yok.
- Bolge secimi su an sadece config dosyasi uzerinden; surukle-birak secim
  arayuzu planlanan bir sonraki adim.
