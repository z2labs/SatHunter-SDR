"""SatHunter FG23 OTG rev A - single source of truth for parts, footprints and nets.

Drawn by gen_sch.py (house schematic style); check_netlist.py proves the drawing equals this table;
twin_check.py checks the netlist against a spec written from the source documents.
"""
import os
HERE = os.path.dirname(os.path.abspath(__file__))
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

