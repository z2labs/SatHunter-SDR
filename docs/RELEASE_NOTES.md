# SatHunterSDR 0.2.0 beta 2

Android satellite-hunting SDR receiver. Install the APK below (allow installs from your browser or
file manager when asked).

## New in beta 2

- **RTL-SDR Blog V4**: the V4 is recognised from its USB product string (Android hands the driver
  only a file descriptor, so the old detection could miss it and set the R828D up with the wrong
  crystal, which made it nearly deaf). The status line under the frequency now says
  "(RTL-SDR Blog V4)" when it was recognised.
- **Tuning digits**: drag a digit of the frequency up or down, or tap its upper / lower half, to
  change it. Long-press a digit or press the keyboard button to type a frequency in MHz.
- **Dragging the spectrum sweeps the frequency axis** (like a map) instead of moving the marker.
  Zoomed in, the view slides first; past the edge the receiver retunes and follows the finger.
  Tap still tunes to the tapped frequency.

## New in beta 1

- **Full-screen mode** (the ⛶ button on the receiver): the waterfall fills the screen, the
  frequency, play/stop and mode buttons float over it and fade away after 4 s; a touch brings them
  back. Swipe from the screen edge for the system bars.
- **RTL-SDR** over USB OTG (RTL2832U + R820T/R828D; 250 kS/s – 2.4 MS/s, gain steps or auto).
- **ESP32-S3 ESP-SDR** over its native USB port: 2204–2804 MHz, 250 / 125 / 62.5 kS/s, gain
  index 0–82, smooth in-stream tuning with firmware that supports it, firmware version shown.
- Plug and play: plugging an SDR in switches to it (Android asks for permission once), unplugging
  goes back to the demo source. Each source remembers its own frequency.

## Since 0.1

- Satellites from SatNOGS DB and CelesTrak, pass list with favourites and Hungarian satellites
  first, Doppler tracking, Look4Sat-style radar with compass and aim mode, simulated passes.
- Demo source: works without hardware.

## Known limits

- ESP-SDR: no wideband spectrum (SPEC) mode and no firmware updater yet; use a board with ESP-SDR
  firmware already on it.
- Decoder plugins: format and catalog only, the runtime comes next.
- Beta: please send feedback with the device model and what you were doing.
