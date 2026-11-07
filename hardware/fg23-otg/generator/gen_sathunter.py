#!/usr/bin/env python3
"""SatHunter FG23 OTG - KiCad 7-format schematic generator (opens in KiCad 7/8/9/10).

One board: SMA -> RF ESD -> SAW1 -> QPL9547 LNA -> SAW2 -> RX match -> EFR32FG23 (RX only, 39 MHz TCXO)
-> SPI 19.5 MHz -> ESP32-S3-WROOM-1-N16R8 (PSRAM ring, TinyUSB CDC) -> USB-C -> Android OTG.

Schematic stage only (rev A, 2026-11-07): placement and routing follow in the layout step
(srouter, same flow as fg23-cw-trainer). Framework (symbols, label style, kfp footprint reader)
taken from fg23-cw-trainer/generator/gen_trainer.py.

Outputs (OUT dir): sathunter_fg23.kicad_pro/.kicad_sch, SH.kicad_sym, SH.pretty/, lib tables, BOM.csv, dnp.txt
Every connection that comes from a datasheet or a reference design says so next to the part.
"""
import uuid, json, os, shutil
PROJECT = "sathunter_fg23"
HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ.get("SH_OUT") or os.path.join(HERE, "..", "kicad")
REV = "A"
os.makedirs(OUT, exist_ok=True)
def U(): return str(uuid.uuid4())
ROOT = U()
import kfp

# ============================================================================ FOOTPRINTS
FPMAP = {
 "R0402": "Resistor_SMD:R_0402_1005Metric", "C0402": "Capacitor_SMD:C_0402_1005Metric", "L0402": "Inductor_SMD:L_0402_1005Metric",
 "R0603": "Resistor_SMD:R_0603_1608Metric", "C0603": "Capacitor_SMD:C_0603_1608Metric", "L0603": "Inductor_SMD:L_0603_1608Metric",
 "C0805": "Capacitor_SMD:C_0805_2012Metric", "L0805": "Inductor_SMD:L_0805_2012Metric",
 "D0402": "Diode_SMD:D_0402_1005Metric", "LED_0603": "LED_SMD:LED_0603_1608Metric",
 "L_2016": "Inductor_SMD:L_Murata_DFE201610P",
 "QFN48": "src_fp/QFN-48-1EP_6x6mm_P0.4mm_EP4.45x4.45mm_SiliconLabs.kicad_mod",
 "DFN8_2x2": "Package_DFN_QFN:DFN-8-1EP_2x2mm_P0.5mm_EP0.9x1.6mm",
 "SAW6": "Filter:Filter_SAW-6_3.8x3.8mm",
 "OSC2520": "Oscillator:Oscillator_SMD_SeikoEpson_TG2520SMN-xxx-xxxxxx-4Pin_2.5x2.0mm",
 "WROOM1": "RF_Module:ESP32-S3-WROOM-1",
 "SOT-23-5": "Package_TO_SOT_SMD:SOT-23-5", "SOT-23-6": "Package_TO_SOT_SMD:SOT-23-6",
 "SMA": "Connector_Coaxial:SMA_Samtec_SMA-J-P-X-ST-EM1_EdgeMount",
 "USBC": "Connector_USB:USB_C_Receptacle_GCT_USB4105-xx-A_16P_TopMnt_Horizontal",
 "TC2030": "Connector:Tag-Connect_TC2030-IDC-NL_2x03_P1.27mm_Vertical",
 "SW_SMD": "Button_Switch_SMD:SW_SPST_PTS810",
 "TP": "TestPoint:TestPoint_Pad_D1.0mm",
}
def fp_lib(fp):
    """KiCad footprint field: official libs by name, project footprints from SH.pretty."""
    spec = FPMAP[fp]
    if spec.endswith(".kicad_mod"): return "SH:" + os.path.basename(spec)[:-10]
    return spec

# ============================================================================ PARTS
# ref: dict(fp, value, pins{padnum: net}, sym, desc, dnp)
PARTS = {}
def part(ref, fp, value, pins, sym="2", desc="", dnp=False):
    if fp in ("0402", "0603", "0805"): fp = {"R": "R", "C": "C", "L": "L", "F": "L"}[ref[0]] + fp
    if fp not in FPMAP: raise KeyError((ref, fp))
    if ref in PARTS: raise KeyError("duplicate " + ref)
    PARTS[ref] = dict(fp=fp, value=value, pins={str(k): v for k, v in pins.items()}, sym=sym, desc=desc, dnp=dnp)

# ---------------------------------------------------------------- RF input chain (spec: claude/SatHunter-FG23-OTG-nyak-spec-2026-11-07.md)
part("J_ANT", "SMA", "SMA edge-mount female (Samtec SMA-J-P-H-ST-EM1)", {1: "ANT", 2: "GND"},
     sym=("box", [("1", "RF"), ("2", "GND")]), desc="footprint as fg23-cw-trainer rev A5")
part("D_ESD", "D0402", "RF ESD, bidirectional, <=0.3 pF (LCSC TBD)", {1: "ANT", 2: "GND"}, desc="low-capacitance RF ESD at the connector")
# SAW: Shoulder HDF434A1-S6, 434 MHz / 10 MHz BW, 50 ohm in/out. Pinout from Spacewalker LNA 434 v1.3 schematic
# (SWL434_1v3_sch.pdf): P$2 IN, P$5 OUT, P$1/3/4/6 GND. Footprint: KiCad SAW-6 3.8x3.8 - VERIFY against the Shoulder drawing.
SAWPINS = [("2", "IN"), ("5", "OUT"), ("1", "GND"), ("3", "GND"), ("4", "GND"), ("6", "GND")]
part("SAW1", "SAW6", "HDF434A1-S6", {2: "ANT", 5: "RF_S1O", 1: "GND", 3: "GND", 4: "GND", 6: "GND"}, sym=("box", SAWPINS),
     desc="Shoulder 434 MHz SAW, as Spacewalker LNA 434; footprint to verify vs maker drawing")
part("R_BYP", "0402", "0R", {1: "ANT", 2: "RF_S1O"}, dnp=True, desc="SAW1 bypass (fit 0R and remove SAW1): ~2 dB lower NF where no strong blockers")
part("C_LNI", "0402", "100p C0G", {1: "RF_S1O", 2: "RF_LNI"}, desc="QPL9547 RF In needs a DC block (datasheet pin 2)")
# QPL9547 (Qorvo), DFN-8 2x2: 1 Vbias, 2 RF In, 3/4/5/8 NC, 6 Shut Down (high = off, low/GND = on), 7 RF Out / VDD, EP = GND.
# Bias R 3.32k as on QPL9547EVB-01 (R4) and Spacewalker; VDD 3.15-5.25 V -> 3.3 V (W1GHZ: best NF near 3.3 V / 3.3k).
part("U_LNA", "DFN8_2x2", "QPL9547TR7", {1: "LNA_VB", 2: "RF_LNI", 3: "GND", 4: "GND", 5: "GND", 6: "LNA_SD", 7: "RF_LNO", 8: "GND", 9: "GND"},
     sym=("box", [("2", "RF_IN"), ("1", "VBIAS"), ("6", "SHDN"), ("3", "NC"), ("4", "NC"), ("7", "RF_OUT/VDD"), ("5", "NC"), ("8", "NC"), ("9", "EP")]),
     desc="LNA ~25 dB, NF ~0.4 dB; NC pins grounded (datasheet: may float or ground); land pattern to verify vs Qorvo drawing")
part("R_BIAS", "0402", "3k32 1%", {1: "V_LNA", 2: "LNA_VB"}, desc="bias current set (EVB R4)")
part("C_BIAS", "0402", "100p C0G", {1: "LNA_VB", 2: "GND"}, desc="as Spacewalker C1")
part("L_CHK", "0603", "560nH wirewound", {1: "V_LNA", 2: "RF_LNO"}, desc="RF choke feeding VDD into RF Out (Spacewalker L1/L3); LCSC check SRF > 600 MHz")
part("C_LNO", "0402", "100p C0G", {1: "RF_LNO", 2: "RF_S2I"}, desc="RF Out DC block")
part("R_SDPD", "0402", "100k", {1: "LNA_SD", 2: "GND"}, desc="LNA on by default (EVB R2 100k); FG23 PB0 high = LNA off")
part("FB_LNA", "0402", "BLM15HG601SN1D 600R", {1: "V3RF", 2: "V_LNA"}, desc="LNA supply filter")
part("C_LNA1", "0402", "1u", {1: "V_LNA", 2: "GND"})
part("C_LNA2", "0402", "100p C0G", {1: "V_LNA", 2: "GND"})
part("SAW2", "SAW6", "HDF434A1-S6", {2: "RF_S2I", 5: "RF_S2O", 1: "GND", 3: "GND", 4: "GND", 6: "GND"}, sym=("box", SAWPINS),
     desc="second SAW: image and blocker rejection ahead of the FG23")
# pi-pad option between SAW2 and the FG23 match: terminates the SAW in 50 ohm (FG23 RX input is high impedance).
# Default: series 0R, shunts not fitted. 3 dB pad: 18R series, 300R shunts (adds ~0.1 dB system NF behind 21 dB).
part("R_PD1", "0402", "300R", {1: "RF_S2O", 2: "GND"}, dnp=True, desc="pi-pad shunt (3 dB option)")
part("R_PDS", "0402", "0R", {1: "RF_S2O", 2: "RF_PD"}, desc="pi-pad series (0R = no pad; 18R for 3 dB)")
part("R_PD2", "0402", "300R", {1: "RF_PD", 2: "GND"}, dnp=True, desc="pi-pad shunt (3 dB option)")
# RX match: AN923.2 Table 4.1 / BRD4265B: RX = single series inductor resonating with the LNA input (82 nH at 434 MHz),
# shunt C at the RX pin not mounted at 434 MHz. TX path (SUBG_O0/O1) unused: RX-only board.
part("C_RXDC", "0402", "220p C0G", {1: "RF_PD", 2: "RF_RXN"}, desc="DC block (BRD4265B C11 value)")
part("C_RXN", "0402", "NM", {1: "RF_RXN", 2: "GND"}, dnp=True, desc="tuning shunt slot")
part("L_RX", "0402", "82n Murata LQW15", {1: "RF_RXN", 2: "SUBG_I0"}, desc="RX match, 434 MHz (AN923.2 Tab. 4.1 L4 / BRD4265B L4)")
part("C_RXS", "0402", "NM", {1: "SUBG_I0", 2: "GND"}, dnp=True, desc="RX shunt slot (AN923.2: used only at 169 MHz)")

# ---------------------------------------------------------------- EFR32FG23 (pin map verified vs BRD4265B/BRD4210A in fg23-cw-trainer)
U1_PINS = {1: "PC0", 2: "PC1", 3: "PC2", 4: "PC3", 5: "PC4", 6: "PC5", 7: "PC6", 8: "PC7", 9: "PC8", 10: "PC9", 11: "HFXTAL_I", 12: "HFXTAL_O",
 13: "RESETn", 14: "RFVDD", 15: "RFVSS", 16: "SUBG_I1", 17: "SUBG_I0", 18: "SUBG_O1", 19: "SUBG_O0", 20: "PAVDD", 21: "PB3", 22: "PB2", 23: "PB1", 24: "PB0",
 25: "PA0", 26: "PA1", 27: "PA2", 28: "PA3", 29: "PA4", 30: "PA5", 31: "PA6", 32: "PA7", 33: "PA8", 34: "PA9", 35: "PA10", 36: "DECOUPLE",
 37: "VREGSW", 38: "VREGVDD", 39: "VREGVSS", 40: "DVDD", 41: "AVDD", 42: "IOVDD", 43: "PD5", 44: "PD4", 45: "PD3", 46: "PD2", 47: "PD1", 48: "PD0", 49: "VSS_PAD"}
# Link pins = the ionos firmware (firmware-fg23/iq_capture: iq_stream.c SPI_MOSI PC00, SPI_CLK PC05, SPI_CS PA07, RDY PD02;
# cmdlink.c CMDLINK_RX PA06; SWO PA03) so the same FG23 image runs on this board.
U1_NET = {1: "MOSI_F", 6: "SCLK_F", 32: "CS_F", 31: "FG_CMD", 46: "FG_RDY", 26: "SWCLK", 27: "SWDIO", 28: "SWO", 24: "LNA_SD",
 11: "HFXI", 13: "RESETn", 14: "RFVDD", 15: "GND", 17: "SUBG_I0", 20: "PAVDD", 36: "DECOUPLE", 37: "VREGSW", 38: "V3RF", 39: "GND",
 40: "VDCDC", 41: "V3RF", 42: "V3RF", 49: "GND"}
part("U_FG23", "QFN48", "EFR32FG23B010F512IM48", {k: U1_NET.get(k, "") for k in U1_PINS},
     sym=("box", [(str(k), U1_PINS[k]) for k in range(1, 50)]),
     desc="RX only; SUBG_O0/O1 and SUBG_I1 unconnected; Silabs land pattern from fg23-cw-trainer. Variant (B010/A020) per JLC stock")
# supply block = fg23-cw-trainer rev A5 values (per BRD4265B) - PAVDD from the DC-DC as on BRD4265B (R211 fitted, 10 dBm config)
part("C_VREG1", "0402", "10u 6V3 X5R", {1: "V3RF", 2: "GND"})
part("C_VREG2", "0402", "100n", {1: "V3RF", 2: "GND"})
part("C_AVDD1", "0402", "1u", {1: "V3RF", 2: "GND"})
part("C_AVDD2", "0402", "10n", {1: "V3RF", 2: "GND"})
part("C_IOVDD1", "0402", "1u", {1: "V3RF", 2: "GND"})
part("C_IOVDD2", "0402", "100n", {1: "V3RF", 2: "GND"})
part("C_DEC", "0402", "1u 6V3 X5R", {1: "DECOUPLE", 2: "GND"})
part("L_DCDC", "L_2016", "2u2 DFE201610E-2R2", {1: "VREGSW", 2: "VDCDC"}, desc="FG23 DC-DC inductor")
part("C_DCDC1", "0402", "4u7 6V3 X5R", {1: "VDCDC", 2: "GND"})
part("C_DCDC2", "0402", "100n", {1: "VDCDC", 2: "GND"})
part("FB_RF", "0402", "BLM03AG700SN1D 70R", {1: "VDCDC", 2: "RFVDD"}, desc="BRD4265B L102")
part("C_RF1", "0402", "1u", {1: "RFVDD", 2: "GND"})
part("C_RF2", "0402", "100p", {1: "RFVDD", 2: "GND"})
part("R_PA", "0402", "0R", {1: "VDCDC", 2: "PAVDD_F"}, desc="BRD4265B R211 (DC-DC -> PAVDD)")
part("C_PA1", "0402", "470n", {1: "PAVDD_F", 2: "GND"})
part("C_PA2", "0402", "100p", {1: "PAVDD_F", 2: "GND"})
part("FB_PA", "0402", "BLM03PG330SN1D 33R", {1: "PAVDD_F", 2: "PAVDD"}, desc="BRD4265B L105")
part("C_RST", "0402", "100n", {1: "RESETn", 2: "GND"})
# TCXO into HFXTAL_I through 1 nF, HFXTAL_O open (clipped sine). Vc slot per the Kolibri VCTCXO plan (12k/10k -> ~1.5 V).
part("Y_TCXO", "OSC2520", "39.000MHz TCXO clipped sine +/-0.5ppm (LCSC TBD)", {1: "TCXO_VC", 2: "GND", 3: "TCXO_OUT", 4: "V_TCXO"},
     sym=("box", [("1", "VC/NC"), ("2", "GND"), ("3", "OUT"), ("4", "VCC")]), desc="4-pad 2520; pin 1 NC on a plain TCXO")
part("C_TCX", "0402", "1n C0G", {1: "TCXO_OUT", 2: "HFXI"}, desc="AC coupling into HFXTAL_I")
part("FB_TCX", "0402", "BLM15HG601SN1D 600R", {1: "V3RF", 2: "V_TCXO"})
part("C_TC1", "0402", "100n", {1: "V_TCXO", 2: "GND"})
part("C_TC2", "0402", "1u", {1: "V_TCXO", 2: "GND"})
part("R_VC1", "0402", "12k", {1: "V_TCXO", 2: "TCXO_VC"}, dnp=True, desc="VCTCXO only: Vc divider top")
part("R_VC2", "0402", "10k", {1: "TCXO_VC", 2: "GND"}, dnp=True, desc="VCTCXO only: Vc divider bottom")
part("C_VC", "0402", "1u", {1: "TCXO_VC", 2: "GND"}, dnp=True, desc="VCTCXO only: Vc filter")

# ---------------------------------------------------------------- FG23 <-> ESP32-S3 link (ionos-mini pin map v2 conventions)
# FG23 is the SPI master. Source termination: 22R at the driver. FG23-driven (SCLK, MOSI, CS) at the FG23,
# ESP-driven (CMD = UART into FG23 PA06, RDY = ESP ready into FG23 PD02) at the ESP. RDY 10k pull-down at the FG23.
part("R_SCLK", "0402", "22R", {1: "SCLK_F", 2: "FG_SCLK"})
part("R_MOSI", "0402", "22R", {1: "MOSI_F", 2: "FG_MOSI"})
part("R_CS", "0402", "22R", {1: "CS_F", 2: "FG_CS"})
part("R_CSPU", "0402", "10k", {1: "V3RF", 2: "FG_CS"}, desc="CS idle high while the FG23 is in reset")
part("R_CMD", "0402", "22R", {1: "CMD_E", 2: "FG_CMD"})
part("R_RDY", "0402", "22R", {1: "RDY_E", 2: "FG_RDY"})
part("R_RDYPD", "0402", "10k", {1: "FG_RDY", 2: "GND"}, desc="RDY low = not ready while the ESP boots")
part("R_FRST", "0402", "22R", {1: "FRST_E", 2: "RESETn"}, desc="ESP GPIO40 drives FG23 RESETn open-drain")
part("J_SWD", "TC2030", "Tag-Connect TC2030-IDC-NL (1 VCC 2 SWDIO 3 RST 4 SWCLK 5 GND 6 SWO)",
     {1: "V3RF", 2: "SWDIO", 3: "RESETn", 4: "SWCLK", 5: "GND", 6: "SWO"},
     sym=("box", [("1", "VCC"), ("2", "SWDIO"), ("3", "RST"), ("4", "SWCLK"), ("5", "GND"), ("6", "SWO")]),
     desc="FG23 debug, same pinout as the fg23-cw-trainer header; the ESP also bit-bangs SWD on the same nets")

# ---------------------------------------------------------------- ESP32-S3-WROOM-1-N16R8 (pins: KiCad RF_Module symbol == QEDA library; 2 sources)
ESP_PINS = {1: "GND", 2: "3V3", 3: "EN", 4: "IO4", 5: "IO5", 6: "IO6", 7: "IO7", 8: "IO15", 9: "IO16", 10: "IO17", 11: "IO18", 12: "IO8",
 13: "IO19", 14: "IO20", 15: "IO3", 16: "IO46", 17: "IO9", 18: "IO10", 19: "IO11", 20: "IO12", 21: "IO13", 22: "IO14", 23: "IO21",
 24: "IO47", 25: "IO48", 26: "IO45", 27: "IO0", 28: "IO35", 29: "IO36", 30: "IO37", 31: "IO38", 32: "IO39", 33: "IO40", 34: "IO41",
 35: "IO42", 36: "RXD0", 37: "TXD0", 38: "IO2", 39: "IO1", 40: "GND", 41: "GND"}
ESP_NET = {1: "GND", 2: "V3D", 3: "ESP_EN", 13: "USB_DM", 14: "USB_DP", 18: "FG_CS", 19: "FG_MOSI", 20: "FG_SCLK", 21: "CMD_E", 22: "RDY_E",
 23: "LED_ST", 25: "SWO", 27: "ESP_BOOT", 31: "SWDIO", 32: "SWCLK", 33: "FRST_E", 36: "U0RXD", 37: "U0TXD", 40: "GND", 41: "GND"}
part("U_ESP", "WROOM1", "ESP32-S3-WROOM-1-N16R8", {k: ESP_NET.get(k, "") for k in ESP_PINS},
     sym=("box", [(str(k), ESP_PINS[k]) for k in range(1, 42)]),
     desc="same module as the ionos-mini DevKit (octal PSRAM: IO35-37 unusable). FG23 link on SPI2 IO_MUX (IO10-12)")
part("R_EN", "0402", "10k", {1: "V3D", 2: "ESP_EN"})
part("C_EN", "0402", "1u", {1: "ESP_EN", 2: "GND"}, desc="EN RC delay (Espressif reference)")
part("SW_RST", "SW_SMD", "Tact SMD (C&K PTS810)", {1: "ESP_EN", 2: "GND"}, sym=("box", [("1", "1"), ("2", "2")]))
part("R_BOOT", "0402", "10k", {1: "V3D", 2: "ESP_BOOT"})
part("SW_BOOT", "SW_SMD", "Tact SMD (C&K PTS810)", {1: "ESP_BOOT", 2: "GND"}, sym=("box", [("1", "1"), ("2", "2")]), desc="IO0 low at reset = USB download mode")
part("C_ESP1", "0805", "22u 6V3 X5R", {1: "V3D", 2: "GND"})
part("C_ESP2", "0402", "100n", {1: "V3D", 2: "GND"})
part("R_LST", "0402", "1k", {1: "LED_ST", 2: "LED_ST_A"})
part("D_LST", "LED_0603", "LED green (status)", {1: "GND", 2: "LED_ST_A"}, desc="1=K 2=A")
part("R_LPW", "0402", "4k7", {1: "V3D", 2: "LED_PW_A"})
part("D_LPW", "LED_0603", "LED red (power)", {1: "GND", 2: "LED_PW_A"}, desc="1=K 2=A")
part("TP_TX", "TP", "TP U0TXD", {1: "U0TXD"}, sym=("box", [("1", "TP")]))
part("TP_RX", "TP", "TP U0RXD", {1: "U0RXD"}, sym=("box", [("1", "TP")]))

# ---------------------------------------------------------------- USB-C, protection, supplies
part("J_USB", "USBC", "USB-C 16p GCT USB4105-GF-A", {"A1": "GND", "B12": "GND", "A12": "GND", "B1": "GND", "A4": "VBUS", "B9": "VBUS", "A9": "VBUS", "B4": "VBUS",
     "A5": "CC1", "B5": "CC2", "A6": "USB_DP", "B6": "USB_DP", "A7": "USB_DM", "B7": "USB_DM", "A8": "", "B8": "", "S1": "GND"},
     sym=("box", [("A1", "GND"), ("B12", "GND"), ("A12", "GND"), ("B1", "GND"), ("A4", "VBUS"), ("B9", "VBUS"), ("A9", "VBUS"), ("B4", "VBUS"), ("A5", "CC1"), ("B5", "CC2"),
                 ("A6", "D+"), ("B6", "D+"), ("A7", "D-"), ("B7", "D-"), ("A8", "SBU1"), ("B8", "SBU2"), ("S1", "SHIELD")]),
     desc="USB 2.0 device (sink), as fg23-cw-trainer rev A5")
part("R_CC1", "0402", "5k1", {1: "CC1", 2: "GND"})
part("R_CC2", "0402", "5k1", {1: "CC2", 2: "GND"})
# USBLC6-2SC6 (KiCad Power_Protection symbol): 1/6 I/O1, 3/4 I/O2, 5 VBUS, 2 GND
part("U_ESD", "SOT-23-6", "USBLC6-2SC6", {1: "USB_DP", 6: "USB_DP", 3: "USB_DM", 4: "USB_DM", 5: "VBUS", 2: "GND"},
     sym=("box", [("1", "I/O1"), ("6", "I/O1"), ("3", "I/O2"), ("4", "I/O2"), ("5", "VBUS"), ("2", "GND")]))
part("FB_VBUS", "0805", "BLM21PG221SN1D 220R 2A", {1: "VBUS", 2: "V5"}, desc="phone OTG supply filter")
part("C_V5A", "0805", "10u 10V X5R", {1: "V5", 2: "GND"})
part("C_V5B", "0402", "100n", {1: "V5", 2: "GND"})
# ESP digital supply: AP2112K-3.3 (1 VIN 2 GND 3 EN 4 NC 5 VOUT), 600 mA for WiFi peaks
part("U_LDOD", "SOT-23-5", "AP2112K-3.3TRG1", {1: "V5", 2: "GND", 3: "V5", 4: "", 5: "V3D"},
     sym=("box", [("1", "VIN"), ("2", "GND"), ("3", "EN"), ("4", "NC"), ("5", "VOUT")]), desc="ESP32-S3 3.3 V")
part("C_LDD1", "0402", "1u 10V", {1: "V5", 2: "GND"})
part("C_LDD2", "0603", "10u 6V3", {1: "V3D", 2: "GND"})
# RF supply: LP5907MFX-3.3 (1 IN 2 GND 3 EN 4 NC 5 OUT), low noise, 250 mA: FG23 + LNA + TCXO
part("U_LDORF", "SOT-23-5", "LP5907MFX-3.3/NOPB", {1: "V5", 2: "GND", 3: "V5", 4: "", 5: "V3RF"},
     sym=("box", [("1", "IN"), ("2", "GND"), ("3", "EN"), ("4", "NC"), ("5", "OUT")]), desc="separate clean 3.3 V for the RF section")
part("C_LDR1", "0402", "1u 10V", {1: "V5", 2: "GND"})
part("C_LDR2", "0402", "1u", {1: "V3RF", 2: "GND"})
part("TP_5V", "TP", "TP V5", {1: "V5"}, sym=("box", [("1", "TP")]))
part("TP_3RF", "TP", "TP V3RF", {1: "V3RF"}, sym=("box", [("1", "TP")]))
part("TP_GND", "TP", "TP GND", {1: "GND"}, sym=("box", [("1", "TP")]))

# every reference ends in a digit (KiCad annotation)
for _r in list(PARTS):
    if not _r[-1].isdigit(): PARTS[_r + "1"] = PARTS.pop(_r)
def R(ref): return ref if ref in PARTS else ref + "1"
NETS = sorted({n for p in PARTS.values() for n in p["pins"].values() if n})

# ============================================================================ SCHEMATIC (symbol machinery from gen_trainer.py)
F = '(effects (font (size 1.27 1.27)))'; FH = '(effects (font (size 1.27 1.27)) hide)'; FS = '(effects (font (size 1.0 1.0)))'
STK = '(stroke (width 0.254) (type default)) (fill (type background))'
def poly(pts): return f'(polyline (pts {" ".join(f"(xy {x} {y})" for x, y in pts)}) (stroke (width 0.254) (type default)) (fill (type none)))'
def rect(x1, y1, x2, y2): return f'(rectangle (start {x1} {y1}) (end {x2} {y2}) {STK})'
def pin(kind, x, y, ang, num, name="~", length=2.54, hide=False):
    h = " hide" if hide else ""
    return f'(pin {kind} line (at {x} {y} {ang}) (length {length}){h} (name "{name}" {FS}) (number "{num}" {FS}))'
SYMS = {}; PINPOS = {}
def add_sym(name, ref, body, pins, pinpos, power=False, shownum=False):
    pw = " (power)" if power else ""; pn = "" if shownum else " (pin_numbers hide)"
    SYMS[name] = (f'(symbol "SH:{name}"{pw}{pn} (pin_names (offset 0.5)) (in_bom yes) (on_board yes)\n'
        f'(property "Reference" "{ref}" (at 0 0 0) {F})\n(property "Value" "{name}" (at 0 0 0) {F})\n'
        f'(property "Footprint" "" (at 0 0 0) {FH})\n(property "Datasheet" "" (at 0 0 0) {FH})\n'
        f'(symbol "{name}_0_1" {" ".join(body)})\n(symbol "{name}_1_1" {" ".join(pins)}))')
    PINPOS[name] = pinpos
add_sym("TwoPin", "U", [rect(-2.54, 1.016, 2.54, -1.016)], [pin("passive", -5.08, 0, 0, "1"), pin("passive", 5.08, 0, 180, "2")], {"1": (-5.08, 0), "2": (5.08, 0)})
SYM_H = {}
def box_sym(name, ref, pinlist):
    n = len(pinlist); nl = (n + 1) // 2; nr = n - nl
    hgt = max(nl, nr) * 2.54 + 2.54; w = 20.32 if n > 2 else 10.16
    body = [rect(-w / 2, hgt / 2, w / 2, -hgt / 2)]
    pins = []; pp = {}
    for i, (num, nm) in enumerate(pinlist[:nl]):
        y = round(hgt / 2 - 2.54 * (i + 1), 3); pins.append(pin("passive", -w / 2 - 2.54, y, 0, num, nm)); pp[num] = (-w / 2 - 2.54, y)
    for i, (num, nm) in enumerate(pinlist[nl:]):
        y = round(hgt / 2 - 2.54 * (i + 1), 3); pins.append(pin("passive", w / 2 + 2.54, y, 180, num, nm)); pp[num] = (w / 2 + 2.54, y)
    add_sym(name, ref, body, pins, pp, shownum=True)
    return hgt, w
SYM_W = {}
for ref, p in PARTS.items():
    if p["sym"] == "2": continue
    kind, pl = p["sym"]; SYM_H[ref], SYM_W[ref] = box_sym("SYM_" + ref, ref.split("_")[0], pl)
add_sym("GND", "#PWR", [poly([(0, 0), (0, -1.27), (1.27, -1.27), (0, -2.54), (-1.27, -1.27), (0, -1.27)])], [pin("power_in", 0, 0, 270, "1", "GND", 0, True)], {"1": (0, 0)}, power=True)
add_sym("PWR_FLAG", "#FLG", [poly([(0, 0), (0, 1.27), (-1.016, 1.905), (0, 2.54), (1.016, 1.905), (0, 1.27)])], [pin("power_out", 0, 0, 90, "1", "pwr", 0, True)], {"1": (0, 0)}, power=True)

sch_items = []; SYM_UUID = {}; PLACED = set()
def g(v): return round(v / 1.27) * 1.27   # snap to the 50 mil grid
def glabel(net, x, y, ang=0):
    just = "left" if ang == 0 else "right"
    return (f'(global_label "{net}" (shape passive) (at {x} {y} {ang}) (fields_autoplaced) (effects (font (size 1.0 1.0)) (justify {just})) (uuid "{U()}")'
            f' (property "Intersheetrefs" "${{INTERSHEET_REFS}}" (at {x} {y} 0) (effects (font (size 1.0 1.0)) hide)))')
def wire(a, b):
    if a != b: sch_items.append(f'(wire (pts (xy {round(a[0],3)} {round(a[1],3)}) (xy {round(b[0],3)} {round(b[1],3)})) (stroke (width 0) (type default)) (uuid "{U()}"))')
def text(t, x, y, size=1.5): sch_items.append(f'(text "{t}" (at {x} {y} 0) (effects (font (size {size} {size})) (justify left bottom)) (uuid "{U()}"))')
def junction(x, y): sch_items.append(f'(junction (at {round(x,3)} {round(y,3)}) (diameter 0) (color 0 0 0 0) (uuid "{U()}"))')
def place(ref, X, Y, wires=True, rot=0):
    """Place a part; with wires=True every pin gets a stub + global label (label style)."""
    p = PARTS[ref]; sid = U(); SYM_UUID[ref] = sid; PLACED.add(ref)
    X, Y = g(X), g(Y)
    name = "TwoPin" if p["sym"] == "2" else "SYM_" + ref
    pins = "\n".join(f'(pin "{n}" (uuid "{U()}"))' for n in p["pins"])
    hh = SYM_H.get(ref, 0)
    ry = Y - (hh / 2 + 2.0) if ref in SYM_H else Y - 2.2
    vy = Y + (hh / 2 + 2.0) if ref in SYM_H else Y + 2.4
    if rot in (90, 270) and ref not in SYM_H: ry, vy = Y, Y
    rx = X + 2.6 if (rot in (90, 270) and ref not in SYM_H) else X
    JL = " (justify left)" if rot in (90, 270) else ""
    sch_items.append(f'(symbol (lib_id "SH:{name}") (at {X} {Y} {rot}) (unit 1) (in_bom yes) (on_board yes) (dnp {"yes" if p["dnp"] else "no"})\n  (uuid "{sid}")\n'
      f'  (property "Reference" "{ref}" (at {rx} {ry - (1.2 if rot in (90, 270) else 0)} 0) (effects (font (size 1.27 1.27)){JL}))\n'
      f'  (property "Value" "{p["value"]}" (at {rx} {vy + (1.2 if rot in (90, 270) else 0)} 0) (effects (font (size 1.0 1.0)){JL}))\n'
      f'  (property "Footprint" "{fp_lib(p["fp"])}" (at {X} {Y} 0) {FH})\n  (property "Datasheet" "~" (at {X} {Y} 0) {FH})\n'
      f'  (property "Description" "{p["desc"]}" (at {X} {Y} 0) {FH})\n  {pins}\n'
      f'  (instances (project "{PROJECT}" (path "/{ROOT}" (reference "{ref}") (unit 1)))))')
    if not wires: return
    pp = PINPOS[name]
    for num, net in p["pins"].items():
        px, py = pp[num]
        if rot == 90: px, py = -py, px      # KiCad: CCW rotation in the symbol's Y-up frame
        elif rot == 270: px, py = py, -px
        sx, sy = X + px, Y - py
        if net == "":
            sch_items.append(f'(no_connect (at {sx} {sy}) (uuid "{U()}"))'); continue
        if px < -0.01: ex = sx - 5.08; wire((sx, sy), (ex, sy)); sch_items.append(glabel(net, ex, sy, 180))
        elif px > 0.01: ex = sx + 5.08; wire((sx, sy), (ex, sy)); sch_items.append(glabel(net, ex, sy, 0))
        else:
            ey = sy - 5.08 if py > 0 else sy + 5.08; wire((sx, sy), (sx, ey)); sch_items.append(glabel(net, sx, ey, 90 if py > 0 else 270))
def pin_xy(ref, num, X, Y, rot=0):
    name = "TwoPin" if PARTS[ref]["sym"] == "2" else "SYM_" + ref
    px, py = PINPOS[name][num]
    if rot == 90: px, py = -py, px
    elif rot == 270: px, py = py, -px
    return (g(X) + px, g(Y) - py)

# ---- RF chain drawn explicitly: series parts in a row, shunts hanging below to GND, boxes (SAW, LNA) via their pins
def gnd_sym(x, y):
    sch_items.append(f'(symbol (lib_id "SH:GND") (at {x} {y} 0) (unit 1) (in_bom yes) (on_board yes) (dnp no)\n  (uuid "{U()}")\n'
      f'  (property "Reference" "#PWR0{len(sch_items)}" (at {x} {y+5} 0) {FH})\n  (property "Value" "GND" (at {x} {y+3.6} 0) {FS})\n'
      f'  (property "Footprint" "" (at {x} {y} 0) {FH})\n  (property "Datasheet" "" (at {x} {y} 0) {FH})\n  (pin "1" (uuid "{U()}"))\n'
      f'  (instances (project "{PROJECT}" (path "/{ROOT}" (reference "#PWR0{len(sch_items)}") (unit 1)))))')
def shunt(ref, x, y_node, drop=10.16):
    """vertical two-pin shunt from (x,y_node) down to GND symbol"""
    cy = y_node + drop
    place(R(ref), x, cy, wires=False, rot=270)            # rot 270: pin 1 on top = the node side, pin 2 to GND
    top = pin_xy(R(ref), "1", x, cy, 270); bot = pin_xy(R(ref), "2", x, cy, 270)
    assert top[1] < bot[1] and PARTS[R(ref)]["pins"]["2"] == "GND", ref
    wire((x, y_node), top); wire(bot, (bot[0], bot[1] + 2.54)); gnd_sym(bot[0], bot[1] + 2.54)
    junction(x, y_node)
def series(ref, x, y):
    place(R(ref), x, y, wires=False); return pin_xy(R(ref), "1", x, y), pin_xy(R(ref), "2", x, y)

RFY = g(45.72); X0 = g(30.48)
text("RF CHAIN  430-440 MHz:  SMA - RF ESD - SAW1 - QPL9547 (~25 dB) - SAW2 - pi-pad option - RX match - EFR32FG23 SUBG_I0.   Spec: SatHunter-FG23-OTG-nyak-spec-2026-11-07", X0, RFY - 22, 1.6)
text("Gain to the FG23 ~21 dB, system NF ~2.6 dB (0.6 dB with SAW1 bypassed). Shield can over SAW1..FG23+TCXO (layout step).", X0, RFY - 18.5, 1.3)
# J_ANT box with labels, then explicit chain from net ANT
place(R("J_ANT"), X0, RFY + 1.27)
xa = g(X0 + 22.86)
sch_items.append(glabel("ANT", xa - 5.08, RFY, 180)); wire((xa - 5.08, RFY), (xa, RFY))
shunt("D_ESD", xa, RFY)
# SAW1 box: label style (ANT / RF_S1O on its pins), bypass resistor drawn above it
place(R("SAW1"), xa + 25.4, RFY + 3.81)
wire((xa, RFY), (xa + 10.16, RFY)); sch_items.append(glabel("ANT", xa + 10.16, RFY, 0))
xb = g(xa + 50.8)
a, b = series("R_BYP", xa + 25.4, RFY - 12.7)
sch_items.append(glabel("ANT", a[0] - 5.08, a[1], 180)); wire((a[0] - 5.08, a[1]), a)
sch_items.append(glabel("RF_S1O", b[0] + 5.08, b[1], 0)); wire(b, (b[0] + 5.08, b[1]))
sch_items.append(glabel("RF_S1O", xb - 2.54, RFY, 180)); wire((xb - 2.54, RFY), (xb, RFY))
a, b = series("C_LNI", xb + 7.62, RFY); wire((xb, RFY), a)
sch_items.append(glabel("RF_LNI", b[0] + 5.08, b[1], 0)); wire(b, (b[0] + 5.08, b[1]))
# LNA block below the row
LX = g(xb + 38.1)
place(R("U_LNA"), LX, RFY + 2.54)
for i, ref in enumerate(("R_BIAS", "C_BIAS", "L_CHK", "R_SDPD", "FB_LNA", "C_LNA1", "C_LNA2")):
    place(R(ref), LX - 76.2 + i * 33.02, RFY + 33.02)     # 27.94 > 10.16 body + 2 x 5.08 label stubs: no touching stubs
xc = g(LX + 30.48)
sch_items.append(glabel("RF_LNO", xc - 2.54, RFY, 180)); wire((xc - 2.54, RFY), (xc, RFY))
a, b = series("C_LNO", xc + 7.62, RFY); wire((xc, RFY), a)
sch_items.append(glabel("RF_S2I", b[0] + 5.08, b[1], 0)); wire(b, (b[0] + 5.08, b[1]))
place(R("SAW2"), xc + 45.72, RFY + 3.81)
xd = g(xc + 71.12)
sch_items.append(glabel("RF_S2O", xd - 2.54, RFY, 180)); wire((xd - 2.54, RFY), (xd, RFY))
shunt("R_PD1", xd, RFY)
a, b = series("R_PDS", xd + 7.62, RFY); wire((xd, RFY), a)
xe = g(b[0] + 5.08); wire(b, (xe, RFY)); shunt("R_PD2", xe, RFY)
a, b = series("C_RXDC", xe + 7.62, RFY); wire((xe, RFY), a)
xf = g(b[0] + 5.08); wire(b, (xf, RFY)); shunt("C_RXN", xf, RFY)
a, b = series("L_RX", xf + 7.62, RFY); wire((xf, RFY), a)
xg = g(b[0] + 5.08); wire(b, (xg, RFY)); shunt("C_RXS", xg, RFY)
sch_items.append(glabel("SUBG_I0", xg + 5.08, RFY, 0)); wire((xg, RFY), (xg + 5.08, RFY))
for nm, x in (("RF_PD", xe), ("RF_RXN", xf)):
    sch_items.append(glabel(nm, x, RFY - 5.08, 90)); wire((x, RFY), (x, RFY - 5.08))

# ---- everything else: titled groups, label style
groups = {
 "EFR32FG23 - RX only (link pins = ionos firmware: PC00 MOSI, PC05 SCLK, PA07 CS, PA06 CMD rx, PD02 RDY, PA03 SWO)": ["U_FG23"],
 "FG23 supplies (fg23-cw-trainer A5 / BRD4265B values; PAVDD from DC-DC as BRD4265B R211)":
    ["C_VREG1", "C_VREG2", "C_AVDD1", "C_AVDD2", "C_IOVDD1", "C_IOVDD2", "C_DEC", "L_DCDC", "C_DCDC1", "C_DCDC2", "FB_RF", "C_RF1", "C_RF2", "R_PA", "C_PA1", "C_PA2", "FB_PA", "C_RST"],
 "39 MHz TCXO -> HFXTAL_I (1 nF), HFXTAL_O open; Vc parts only for a VCTCXO": ["Y_TCXO", "C_TCX", "FB_TCX", "C_TC1", "C_TC2", "R_VC1", "R_VC2", "C_VC"],
 "FG23 <-> ESP32-S3 link: 22R at the driver; FG23 = SPI master": ["R_SCLK", "R_MOSI", "R_CS", "R_CSPU", "R_CMD", "R_RDY", "R_RDYPD", "R_FRST", "J_SWD"],
 "ESP32-S3-WROOM-1-N16R8 (SPI2 IO_MUX slave, TinyUSB CDC, SWD bit-bang to the FG23)": ["U_ESP"],
 "ESP support": ["R_EN", "C_EN", "SW_RST", "R_BOOT", "SW_BOOT", "C_ESP1", "C_ESP2", "R_LST", "D_LST", "R_LPW", "D_LPW", "TP_TX", "TP_RX"],
 "USB-C (OTG device) + ESD + supplies: V5 -> AP2112K (ESP) / LP5907 (RF)": ["J_USB", "R_CC1", "R_CC2", "U_ESD", "FB_VBUS", "C_V5A", "C_V5B", "U_LDOD", "C_LDD1", "C_LDD2", "U_LDORF", "C_LDR1", "C_LDR2", "TP_5V", "TP_3RF", "TP_GND"],
}
cols = [[30.48, 125.0, 205.0], [215.9, 125.0, 390.0], [401.32, 125.0, 570.0]]   # three columns, clear of the title block
for title, refs in groups.items():
    col = min(cols, key=lambda c: c[1]); gx, gy, xl = col
    text(title, gx, gy - 6, 1.6)
    x, y = gx, gy; rowh = 0
    for ref in [R(r) for r in refs]:
        h = (SYM_H[ref] + 8) if ref in SYM_H else 14
        w = (SYM_W[ref] + 34) if ref in SYM_H else 36
        if x + w > xl and x > gx: x = gx; y += rowh + 6; rowh = 0
        place(ref, x + w / 2, y + h / 2); x += w; rowh = max(rowh, h)
    col[1] = y + rowh + 18
missing = set(PARTS) - PLACED
assert not missing, missing
def psym(lib, ref, at):
    X, Y = at
    sch_items.append(f'(symbol (lib_id "SH:{lib}") (at {X} {Y} 0) (unit 1) (in_bom yes) (on_board yes) (dnp no)\n  (uuid "{U()}")\n'
      f'  (property "Reference" "{ref}" (at {X} {Y+6} 0) {FH})\n  (property "Value" "{lib}" (at {X} {Y+4.5} 0) {FS})\n'
      f'  (property "Footprint" "" (at {X} {Y} 0) {FH})\n  (property "Datasheet" "" (at {X} {Y} 0) {FH})\n  (pin "1" (uuid "{U()}"))\n'
      f'  (instances (project "{PROJECT}" (path "/{ROOT}" (reference "{ref}") (unit 1)))))')
PX = 500.38
psym("GND", "#PWR01", (PX, 40.64)); sch_items.append(glabel("GND", PX, 34.29, 90)); wire((PX, 40.64), (PX, 34.29))
psym("PWR_FLAG", "#FLG01", (PX + 10.16, 40.64)); wire((PX + 10.16, 40.64), (PX + 10.16, 34.29)); sch_items.append(glabel("GND", PX + 10.16, 34.29, 90))
psym("PWR_FLAG", "#FLG02", (PX + 20.32, 40.64)); wire((PX + 20.32, 40.64), (PX + 20.32, 34.29)); sch_items.append(glabel("VBUS", PX + 20.32, 34.29, 90))
gnd_label_y = None
# the GND symbols of the explicit RF shunts join the GND net through the power symbol's name

sch = [f'(kicad_sch (version 20230121) (generator eeschema)\n(uuid "{ROOT}")\n(paper "A2")']
sch.append(f'(title_block (title "SatHunter FG23 OTG - 70 cm satellite receiver") (date "2026-11-07") (rev "{REV}") (company "HA7DCD / Z2 Labs")'
           f' (comment 1 "ANT-SAW-QPL9547-SAW-EFR32FG23 (RX) - ESP32-S3-WROOM-1 - USB-C OTG to the SatHunterSDR Android app")'
           f' (comment 2 "generated by generator/gen_sathunter.py - do not hand-edit; label-style, RF chain drawn explicitly"))')
sch.append("(lib_symbols\n" + "\n".join(SYMS.values()) + "\n)")
sch.extend(sch_items)
sch.append('(sheet_instances (path "/" (page "1")))\n)')
open(os.path.join(OUT, PROJECT + ".kicad_sch"), "w").write("\n".join(sch))

# ============================================================================ LIBS, PROJECT, BOM
os.makedirs(os.path.join(OUT, "SH.pretty"), exist_ok=True)
for fp, spec in FPMAP.items():
    if spec.endswith(".kicad_mod"): shutil.copy(os.path.join(HERE, spec), os.path.join(OUT, "SH.pretty", os.path.basename(spec)))
open(os.path.join(OUT, "SH.kicad_sym"), "w").write('(kicad_symbol_lib (version 20220914) (generator kicad_symbol_editor)\n' +
    "\n".join(v.replace('(symbol "SH:', '(symbol "', 1) for v in SYMS.values()) + '\n)\n')
open(os.path.join(OUT, "fp-lib-table"), "w").write('(fp_lib_table\n  (version 7)\n  (lib (name "SH")(type "KiCad")(uri "${KIPRJMOD}/SH.pretty")(options "")(descr "SatHunter FG23 project footprints"))\n)\n')
open(os.path.join(OUT, "sym-lib-table"), "w").write('(sym_lib_table\n  (version 7)\n  (lib (name "SH")(type "KiCad")(uri "${KIPRJMOD}/SH.kicad_sym")(options "")(descr "SatHunter FG23 symbols"))\n)\n')
pro = {"meta": {"filename": PROJECT + ".kicad_pro", "version": 1},
 "net_settings": {"classes": [
   {"name": "Default", "clearance": 0.127, "track_width": 0.15, "via_diameter": 0.6, "via_drill": 0.3, "diff_pair_gap": 0.15, "diff_pair_via_gap": 0.25, "diff_pair_width": 0.2,
    "line_style": 0, "microvia_diameter": 0.3, "microvia_drill": 0.1, "wire_width": 6, "bus_width": 12, "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)"},
   {"name": "RF", "clearance": 0.127, "track_width": 0.35, "via_diameter": 0.6, "via_drill": 0.3, "diff_pair_gap": 0.15, "diff_pair_via_gap": 0.25, "diff_pair_width": 0.2,
    "line_style": 0, "microvia_diameter": 0.3, "microvia_drill": 0.1, "wire_width": 6, "bus_width": 12, "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)"},
   {"name": "USB", "clearance": 0.127, "track_width": 0.2, "via_diameter": 0.6, "via_drill": 0.3, "diff_pair_gap": 0.15, "diff_pair_via_gap": 0.25, "diff_pair_width": 0.2,
    "line_style": 0, "microvia_diameter": 0.3, "microvia_drill": 0.1, "wire_width": 6, "bus_width": 12, "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)"},
   {"name": "Power", "clearance": 0.127, "track_width": 0.3, "via_diameter": 0.6, "via_drill": 0.3, "diff_pair_gap": 0.15, "diff_pair_via_gap": 0.25, "diff_pair_width": 0.2,
    "line_style": 0, "microvia_diameter": 0.3, "microvia_drill": 0.1, "wire_width": 6, "bus_width": 12, "pcb_color": "rgba(0, 0, 0, 0.000)", "schematic_color": "rgba(0, 0, 0, 0.000)"}],
  "netclass_patterns": [{"netclass": "RF", "pattern": p} for p in ["ANT", "RF_*", "SUBG_*"]] + [{"netclass": "USB", "pattern": p} for p in ["USB_DP", "USB_DM"]] +
                       [{"netclass": "Power", "pattern": p} for p in ["VBUS", "V5", "V3D", "V3RF", "VDCDC", "VREGSW", "PAVDD*", "RFVDD", "V_LNA"]],
  "meta": {"version": 3}},
 "schematic": {"legacy_lib_dir": "", "legacy_lib_list": []}, "sheets": [[ROOT, ""]], "text_variables": {}}
json.dump(pro, open(os.path.join(OUT, PROJECT + ".kicad_pro"), "w"), indent=2)
open(os.path.join(OUT, "dnp.txt"), "w").write("\n".join(r for r, p in PARTS.items() if p["dnp"]) + "\n")
bom = ["Ref,Value,Footprint,DNP,Notes"]
for ref, p in PARTS.items(): bom.append(f'{ref},"{p["value"]}",{fp_lib(p["fp"])},{"DNP" if p["dnp"] else ""},"{p["desc"]}"')
open(os.path.join(OUT, "BOM.csv"), "w").write("\n".join(bom) + "\n")
print("generated", PROJECT, "parts", len(PARTS), "nets", len(NETS), "->", os.path.abspath(OUT))
