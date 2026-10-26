# Decoder plugins

A plugin turns frames from a satellite into something to look at: text, numbers, a map pin or a
picture. It is a folder under `plugins/`:

```
plugins/my-cubesat/
  plugin.toml      name, which satellites and modes it decodes
  decoder.lua      what to show (Luau, the Lua dialect Roblox uses)
  test/            frames and the expected output; CI checks them
```

The app does the radio work (demodulation, bit sync, AX.25 / HDLC framing, CRC). Your script
only gets the decoded frames, so a decoder is usually 10–30 lines.

## plugin.toml

```toml
id = "my-cubesat"          # lower case, digits and '-'
name = "My CubeSat telemetry"
version = "0.1"
author = "7.b, Mezőberény"
description = "Battery voltage and temperature from the beacon."
level = "easy"             # easy / medium / advanced
chain = "afsk1200"         # audio, afsk1200, fsk9600, gmsk, bpsk, cw, iq
baud = 9600                # needed for gmsk and bpsk
norad = [12345]            # satellites; empty = any satellite with a matching mode
modes = ["AFSK"]           # SatNOGS modes
```

## decoder.lua

```lua
function on_frame(frame)
  -- frame.src, frame.dst, frame.path, frame.info (text), frame.bytes (all bytes)
  show.text(frame.src .. ": " .. frame.info)
  local volts = bytes.u16le(frame.bytes, 17) / 1000
  show.value("Battery", volts, "V")
end
```

Available to scripts (sandboxed: no files, no network):

| Call | What it does |
| --- | --- |
| `show.text(s)` | a line in the decoder log |
| `show.value(name, number, unit)` | a live value, also plotted over the pass |
| `show.map(lat, lon, label)` | a pin on the map |
| `show.image(width, height, pixels)` | a picture (SSTV, camera beacons) |
| `bytes.u8 / u16le / u16be / s16le / u32le (b, i)` | read numbers from frame bytes (1-based) |
| `aprs.position(info)` | latitude, longitude from an APRS position report |
| `on_audio(samples)` | for `chain = "audio"`: 48 kHz audio blocks |
| `on_start(info)` / `on_end()` | the pass begins / ends |

## Adding to the catalog

1. Add the plugin folder and a test (`test/frames.txt`, `test/expected.txt`).
2. Add the satellite profile and the plugin to `catalog/index.json`.
3. Open a pull request. CI validates the manifest and the catalog and runs the plugin on its test data.

Status: the manifest and catalog format are in place (`core/crates/sh-plugin`); the Luau runtime
and the AFSK1200 / AX.25 chain are the next step.
