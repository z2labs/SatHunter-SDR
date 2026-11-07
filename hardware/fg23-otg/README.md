# SatHunter FG23 OTG receiver, rev A (schematic)

One board instead of the Spacewalker LNA + RTL-SDR Blog V3 chain: a 70 cm (430-440 MHz) satellite receiver that plugs
into an Android phone over USB-C OTG and streams 16-bit I/Q to the SatHunterSDR app.

```
SMA - RF ESD - SAW1 - QPL9547 - SAW2 - [pi-pad] - RX match - EFR32FG23 (RX, 39 MHz TCXO)
      -> SPI 19.5 MHz -> ESP32-S3-WROOM-1-N16R8 (PSRAM ring, TinyUSB CDC) -> USB-C -> phone
```

**Status:** schematic generated, netlist self-check 0 problems, digital twin layer 1: 50 pass, 0 FAIL, self-test 8/8.
Placement and routing are the next step (srouter, same flow as fg23-cw-trainer). Not fabricated or measured.

Schematic: [kicad/sathunter_fg23_schematic.pdf](kicad/sathunter_fg23_schematic.pdf). BOM with design notes: [kicad/BOM.csv](kicad/BOM.csv).

## Budget (design values)

| | |
|---|---|
| Gain to the FG23 | ~21 dB (SAW 2 dB, QPL9547 ~25 dB, SAW 2 dB) |
| System NF | ~2.6 dB; ~0.6 dB with SAW1 bypassed (R_BYP1 fitted, SAW1 removed) |
| FG23 NF sensitivity | the system NF moves < 0.1 dB for an FG23 NF of 5-9 dB |
| Supply from the phone | < 150 mA (QPL9547 at 3.3 V, FG23 RX, ESP32-S3 with WiFi off) |
| I/Q to the phone | 50-200 ksps int16 (ionos-mini measurement, 9 h soak at 200 ksps) |

## Design decisions

- **One LNA stage.** The Spacewalker's second stage adds 0.05 dB of NF and costs ~23 dB of dynamic range in front of the FG23.
- **QPL9547 at 3.3 V** from the RF LDO, bias 3.32k (QPL9547EVB-01 R4; W1GHZ measured the best 432 MHz NF near 3.3 V / 3.3k).
  Shut-down pin pulled low (LNA on), FG23 PB00 can switch it off.
- **RX-only FG23 match.** AN923.2 Table 4.1 / BRD4265B: the RX path is one series 82 nH resonating with the LNA input,
  shunt C not mounted at 434 MHz. SUBG_O0/O1 (PA) and SUBG_I1 are unconnected. A 220 pF DC block keeps the SAW and pad off
  the RX pin. The FG23 input is high impedance, so SAW2 does not see 50 ohm: the pi-pad footprint (0R fitted, shunts not
  fitted) can become a 3 dB pad (18R / 300R / 300R) if SAW2 ripple turns out to matter; it costs ~0.1 dB of system NF.
- **TCXO, not a crystal.** 39.000 MHz clipped sine into HFXTAL_I through 1 nF, HFXTAL_O open. Doppler tracking at 435 MHz
  needs it (1 ppm = 435 Hz). Vc parts (12k/10k/1u) are not fitted; they are for a VCTCXO, per the Kolibri plan.
- **ESP32-S3-WROOM-1-N16R8**, the module on the ionos-mini DevKit, so the IDF firmware with the 2 MB PSRAM ring runs unchanged.
  (The concept render showed a MINI-1; the WROOM-1 is 18 x 25.5 mm, so the board grows to about 30 x 70 mm.)
- **FG23 pins = the ionos firmware**: PC00 MOSI, PC05 SCLK, PA07 CS, PA06 CMD (rx), PD02 RDY (driven by the ESP), PA03 SWO.
  ESP side = ionos-mini pin map v2: GPIO12 SCLK, 11 MOSI, 10 CS (SPI2 IO_MUX), 13 CMD, 14 RDY, 38/39/48/40 SWD + RESETn.
  22R source termination at the driver, RDY 10k pull-down, CS 10k pull-up.
- **Two LDOs from VBUS**: AP2112K-3.3 for the ESP32-S3, LP5907-3.3 for the FG23, LNA and TCXO (each through its own ferrite).
- **FG23 programming**: the ESP bit-bangs SWD (firmware update from the phone), plus a TC2030 pad set with the
  fg23-cw-trainer pinout.

## To verify before layout

- HDF434A1-S6 land pattern and pin numbering against the Shoulder drawing (KiCad SAW-6 3.8 x 3.8 used; pins 2 IN / 5 OUT from the Spacewalker schematic).
- QPL9547 DFN-8 2 x 2 land pattern and paddle size against the Qorvo drawing.
- 560 nH choke: wirewound 0603 with an SRF well above 440 MHz (LCSC check).
- TCXO and RF ESD part numbers: chosen in JLC's tool (LCSC codes are checked there, not from memory).
- Run ERC in KiCad 10 (KiCad 7 has no command-line ERC; the netlist checks below replace it here).
- Shield can (SAW1 ... FG23 + TCXO): frame size chosen at placement.

## Files

```
generator/gen_sathunter.py   the schematic is generated from Python (parts, nets, symbols, BOM); do not hand-edit kicad/
generator/check_netlist.py   KiCad-extracted netlist == the generator's part table (catches touching label stubs, shorts)
generator/twin_check.py         digital twin layer 1 (guide 2026-11-07: twin_check.py on the .net before placement): netlist against a spec written from the datasheets / firmware / pin map
generator/kfp.py, src_fp/    footprint reader and the Silabs QFN48 land pattern (from fg23-cw-trainer)
kicad/                       KiCad project (opens in KiCad 7-10), SH.kicad_sym, SH.pretty, BOM.csv, dnp.txt, schematic PDF
```

Rebuild and check:
```
python3 generator/gen_sathunter.py
kicad-cli sch export netlist --format kicadsexpr -o kicad/netlist.net kicad/sathunter_fg23.kicad_sch
python3 generator/check_netlist.py
python3 generator/twin_check.py && python3 generator/twin_check.py --selftest
kicad-cli sch export pdf -o kicad/sathunter_fg23_schematic.pdf kicad/sathunter_fg23.kicad_sch
```

Hardware license: CERN-OHL-P-2.0 (the app in this repository is GPL-3.0-or-later).
