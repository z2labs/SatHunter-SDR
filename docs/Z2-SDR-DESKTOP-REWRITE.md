# Z2 SDR desktop: clean rewrite brief

Handoff for the chat that owns the Z2 SDR desktop app. Written 2026-10-26.

## Why

Z2 SDR has been an SDR++ fork (`z2labs/SDRPlusPlus`, branch `esp-sdr`, releases `esp-v0.3.x`).
The SDR++ author does not want AI-rewritten versions of his code around, so the fork will be
deleted. Z2 SDR is rewritten from scratch, without SDR++ code, on the same core as the new
Android app **SatHunterSDR** (`z2labs/SatHunter-SDR`). This time the work goes mobile first,
then desktop.

## Ground rules (clean room)

- Do not copy, translate or paraphrase SDR++ source. Do not open SDR++ files while writing the
  new code. Work only from the feature descriptions in this brief and from our own code.
- Our own code may be reused freely: the ESP-SDR module (`z2labs/sdrpp-esp-sdr-source`:
  `esp_sdr_client.cpp`, `esp_flasher.cpp`, `serial_android.cpp`), the Cubesat Hunter logic, the
  demo source. Those were written for this project and are being ported to Rust in the shared core.
  Do not reuse the SDR++ glue around them (module API, ImGui menus).
- No "SDR++" name, icon or wording anywhere. The app is **Z2 SDR**. Package names and IDs are new.
- License: GPL-3.0-or-later (same as the core).
- Third-party code keeps its own license headers: `sgp4` (MIT), `rustfft` (MIT/Apache-2.0),
  `nusb` (MIT/Apache-2.0), `rtl-sdr-rs` (MPL-2.0) if its register code is used.

## Architecture

One Rust core, two front ends.

```
z2labs/SatHunter-SDR
  core/                       Rust workspace (shared by both apps)
    sh-sat      SatNOGS / CelesTrak parsing, SGP4 orbits, passes, Doppler tracker, locator
    sh-dsp      spectrum FFT, channel extraction, NFM/WFM/AM/USB/LSB/CW demodulators
    sh-sources  Source trait, demo (simulated RTL-SDR); next: RTL-SDR, ESP-SDR, rtl_tcp ...
    sh-plugin   decoder plugin manifests, satellite catalog
    sh-engine   source + DSP thread + satellite tracker + settings, JSON command API
    sh-android  JNI bridge (class io.z2labs.sathunter.Native); also works on a desktop JVM
  android/                    SatHunterSDR app (Kotlin, Jetpack Compose)
  catalog/, plugins/          satellite profiles and decoder plugins
```

The UI talks to the engine through three calls:

- `command(json) -> json`: start/stop, tune, mode, bandwidth, gain, rate, FFT size, squelch, station / locator / GPS, satellite filter, favourites, select, track, simulate, status, sat_list, sat_detail.
- `spectrumRows(float[])`: new FFT rows for the waterfall, in dBFS.
- `audio(float[])`: 48 kHz mono audio.

### Recommended desktop front end

**Compose Multiplatform (Kotlin, desktop JVM).** The Android UI code (screens, radar, satellite
list, settings) is shared with SatHunterSDR. The same `libsathunter` is built for Windows, macOS
and Linux, and loaded over JNI. Draw the waterfall with Skia, or with OpenGL through LWJGL if Skia
is too slow at 4096 bins. Package it with `jpackage` (MSI, DMG, DEB).

Alternative if a JVM is unwanted: a pure Rust UI (egui or Slint) that calls `sh-engine` directly.
It is faster to start but shares no UI code with Android.

Suggested layout: a new repository `z2labs/Z2-SDR` that depends on the core through a pinned git
dependency or a submodule. Alternatively, a `desktop/` folder in the SatHunter-SDR repo.

USB on desktop: `nusb` (pure Rust; Windows WinUSB, macOS IOKit, Linux usbfs). It also opens the
Android USB file descriptor (`Device::from_fd`), so one driver codebase serves both platforms.
For the ESP32-S3 on desktop, its CDC-ACM serial port can be used instead (`serialport` crate).

## Feature list to carry over

All of these exist in the old fork and must come back in Z2 SDR. Items marked ✅ are already in
the shared core with tests.

### Sources

- **ESP-SDR (ESP32-S3)**, the main source:
  - 2204–2804 MHz tuning range; the GUI follows it (band overlay, start at the ISM centre).
  - IQ stream at 250/125/62.5 kS/s, IQS1 frames with CRC32. 8-bit link at ≥250 kS/s, 16-bit below.
  - On-chip spectrum mode (SPEC) at 16/40/80 MHz span, 256/1024/2048 bins, and 4096 bins on PSRAM boards. Mean or max-hold detector, DC fix. Display only (no IQ).
  - Smooth in-stream retune (firmware `iq tune`), throttled to 100 ms while dragging. Sub-kHz fine tuning with an NCO below the S3's 1 kHz LO step. PPM correction. Gain index 0–82.
  - Firmware version query (`VERSION?`), bundled firmware, and a ROM flasher (reset into the bootloader with DTR/RTS) that restarts the board after an update.
  - Connect on a worker thread (no UI freeze). Choose the board's native USB port (303A:1001) and start on plug-in. Recognise the UART bridge (CH343/CP210x/FTDI) and tell the user to use the other USB-C port.
  - Serial port opened with DTR/RTS deasserted. Restart streams that the firmware ended, with a restart-loop guard.
- **RTL-SDR**: open each device once, hot-plug, start/stop tracing. Device and refresh usable while running.
- **Demo (simulated RTL-SDR)** ✅: FM broadcast 88.6/94.8/100.0/104.3 MHz, airband AM 127.5, APRS 144.800, CW beacon 432.400, ISM 433.920, repeater 439.300, plus the selected satellite at its Doppler frequency. Starts automatically when no SDR is present. Switches to the real SDR when one appears.
- Also wanted: rtl_tcp, SpyServer, HackRF, Airspy, Airspy HF+, HydraSDR, PlutoSDR.

### Receiver and UI

- Spectrum and waterfall with automatic range (Auto button, on by default) and an adjustable split between them.
- Frequency display with a fixed strip.
- Demodulators NFM/WFM/AM/USB/LSB/CW ✅, bandwidth, squelch.
- Source and sample rate can be changed while running.
- Play on start. 433.92 MHz on the very first start (or the ESP ISM centre with the ESP source).
- Desktop: an auto-hide menu that slides in over the spectrum from the left edge, with an option to pin it or turn it off.
- Android extras, for reference (SatHunterSDR covers them): touch drawer and rail, pinch zoom, keep the screen on, wrong-USB-port card.
- **Spectrum Export**: FFT to CSV as a snapshot, an average or max hold over N frames, or a continuous log at an interval. Full or visible span.
- **Debug report**: logs, settings, device and USB details in one file (desktop: saved to Downloads; Android: share sheet). Rotating log file.
- **rigctl server** (Hamlib protocol on port 4532), so Gpredict can drive it.
- About section with license and source links. Version and git revision shown in the UI.
- Packages: Windows installer, macOS app, Debian/Ubuntu `.deb` named `z2sdr`, Linux install with the bundled ESP firmware.

### Cubesat Hunter (satellite tracking)

The logic is in the shared core ✅ (`sh-sat`, `sh-engine`):

- SatNOGS DB transmitters, satellites and TLEs, plus CelesTrak amateur TLEs; the newest element set wins. Cached, with an automatic refresh every N days (default 3).
  - Note: SatNOGS uses satellite status `"in orbit"`, not `"alive"`. The old module treated every satellite as dead because of this.
- Filters: band (2 m, 70 cm, 23 cm, S-band 2.2G, 13 cm 2.4G, HF, other), amateur only, active only, favourites only, minimum pass elevation, search.
  - SatNOGS "Unknown" service inside an amateur band counts as amateur.
- Favourites (star) are always listed and come first. Hungarian satellites (SatNOGS `countries` contains `HU`) come next, tagged `[HU]`. Then satellites overhead now, then by AOS.
- Selecting a satellite picks its first matching downlink, sets the demodulator from the mode, and starts Doppler tracking (first order, 10 Hz retune step).
  - Below the horizon it waits on the AOS frequency.
  - A manual retune is kept as an offset in the satellite's frame, so a linear transponder can be tuned across.
  - A big band jump re-centres the hardware 0.2·fs below the signal (away from the DC spike).
- Sky radar:
  - The whole pass, with the part already flown dimmed and a tick every minute.
  - AOS/LOS clock times at the ends and the max elevation at the top.
  - The satellite as a dot, plus up to 12 other satellites overhead.
  - Draggable; tap to switch between small and large.
  - On Android: the compass turns the radar (rotation vector sensor, magnetic declination). Raising the phone switches to aim mode with a crosshair, right/up guidance and ON TARGET within 10°.
- Ground station: latitude, longitude and altitude, or a Maidenhead locator ✅.
  - Desktop: Windows Location Services (a hidden PowerShell call to `GeoCoordinateWatcher`), or "Locate by IP" on request.
  - Android: follow the phone's GPS.
- "Simulate this pass now": shifts the clock to just before AOS, shows SIMULATED, and returns to real time after LOS. Together with the demo source this shows Doppler tracking without an antenna.
- Accuracy checks ✅: ISS passes vs Skyfield within 3 s (AOS/LOS) and 0.05° (max elevation); az/el within 0.02°; range rate within 0.002 km/s.

### Decoder plugins and catalog (new, shared)

- A plugin is a folder: `plugin.toml` + `decoder.lua` (Luau, sandboxed) + `test/`.
  - The heavy DSP stays native: AFSK1200/AX.25, FSK9600, GMSK, BPSK, CW. The script interprets frames and shows text, values, maps and images.
  - WASM plugins are planned for advanced users.
- `catalog/index.json` holds satellite profiles (the ISS: APRS 145.825, voice/SSTV 145.800; Mauve [HU]: 401.75, 2250, 137.925 MHz) and plugin entries.
- New plugins and profiles come in by pull request. CI validates the manifest and runs the plugin on its test data.

## Retiring the old fork

1. Before deleting, keep a private archive: `git clone --mirror` of `z2labs/SDRPlusPlus` and `z2labs/sdrpp-esp-sdr-source`, plus the release assets.
2. GPL: those releases distributed binaries. Delete the binaries together with the source; do not keep offering binaries without source. Our own ESP-SDR client, flasher and Cubesat Hunter code remain ours to reuse.
3. Delete the repository on GitHub (Settings → Danger Zone; only an owner can do this).
4. The Z2 SDR web pages and blog posts that link to the fork must point to the new repository.

## Status (2026-10-26)

- `z2labs/SatHunter-SDR`: the Rust core builds, and 25 unit tests pass (sat, dsp, demo source, plugin catalog, engine end to end).
- Android app: in progress. A GitHub Actions APK build is being set up.
- Next in the core: ESP-SDR and RTL-SDR drivers over `nusb`, the Luau plugin runtime, the AFSK1200/AX.25 chain.
