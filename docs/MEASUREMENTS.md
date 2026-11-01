# Field measurements

Recordings decoded with `cargo run --release -p sh-dsp --example cw_decode -- audio.f32`
(audio from a screen recording: `ffmpeg -i rec.mp4 -vn -ac 1 -ar 48000 -f f32le audio.f32`).

## 2026-11-01 10:41 local, ITUPSAT 1, 70 cm CW beacon

Setup: 6-element 70 cm Yagi (no rear reflector), SkywalkerLNA (bias tee on), RTL-SDR v3,
Xiaomi 10 Pro, app with Doppler tracking on, USB 2.8 kHz, max elevation ~19 deg.

| | |
|---|---|
| Decoded | `(1)KS ITUPSAT HR` · ~1 s carrier · `E` · `TA1KS ITUPS(AT…)` (fade-out at the end) |
| Speed | 12.2 WPM (dot 98 ms); letter gaps ~3.7 units, word gaps ~11 units |
| Signal | 21 dB over the noise in 105 Hz, C/N0 about 41 dB-Hz (lower bound: measured after the app's audio AGC and the recorder's AAC) |
| Offset | the beat tone 2325 Hz at the start (USB): the carrier 2.3 kHz (5.3 ppm) above the tracked frequency. An RTL-SDR v3 TCXO is within ~1 ppm, so most of it is the satellite's transmitter offset and / or the TLE |
| Residual drift | 2325 -> 2219 Hz in 30 s, -2.2 Hz/s on average (-3.7 Hz/s in the first 15 s): the predicted Doppler slope is a little off (TLE age); the tracker's offset (drag the VFO onto the trace) takes care of the constant part |
| Display | the beacon was right under the VFO line on the waterfall (fixed: the line is faint there now) |
