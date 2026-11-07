#!/usr/bin/env python3
"""Digital twin layer 1 for SatHunter FG23 OTG: the exported netlist against a specification written from the
source documents, NOT from the generator's PARTS table (that comparison is check_netlist.py).

Sources, per section:
  [DS-FG23]  EFR32FG23 QFN48 pin numbers as on the BRD4265B schematic (SUBG_I0 17, SUBG_I1 16, SUBG_O0 19, SUBG_O1 18,
             RFVDD 14, RFVSS 15, PAVDD 20, HFXTAL_I 11, HFXTAL_O 12; ports as in fg23-cw-trainer twin spec_cw.py)
  [FW]       ionos-sdr firmware-fg23/iq_capture: iq_stream.c SPI_MOSI PC00, SPI_CLK PC05, SPI_CS PA07, RDY PD02 (input,
             driven by the ESP32); cmdlink.c CMDLINK_RX PA06; SWO PA03
  [MAP]      ionos-mini pin map v2 (2026-10-09): ESP GPIO12 SCLK, 11 MOSI, 10 CS, 13 CMD, 14 RDY, 38 SWDIO, 39 SWCLK,
             48 SWO, 40 RESETn (open drain); 22R at the driver; RDY 10k pull-down
  [DS-ESP]   ESP32-S3-WROOM-1 module pins: QEDA library espressif_esp32-s3-wroom-1.yaml (independent of the KiCad library)
  [DS-QPL]   QPL9547 data sheet: 1 Vbias, 2 RF In, 6 Shut Down (high = off), 7 RF Out/VDD, 9 = paddle GND
  [AN923]    AN923.2 Table 4.1, 434 MHz: RX inductor 82 nH, RX shunt C not mounted
  [SWL]      Spacewalker LNA 434 v1.3 schematic: HDF434A1-S6 pin 2 IN, pin 5 OUT
  [USB]      USB Type-C spec: 5.1k Rd on CC1 and CC2 for a sink; D+ = A6/B6, D- = A7/B7
Run:  python3 twin_check.py [netlist.net]      self-test: python3 twin_check.py --selftest
Exit: 0 pass, 1 FAIL, 3 self-test failed.
"""
import sys, os, copy
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE); import kfp

# ---------------------------------------------------------------- specification
FG = "U_FG23"; ESP = "U_ESP1"; LNA = "U_LNA1"
FG_PIN = {"PC00": "1", "PC05": "6", "PA01": "26", "PA02": "27", "PA03": "28", "PA06": "31", "PA07": "32", "PB00": "24", "PD02": "46",
          "HFXTAL_I": "11", "HFXTAL_O": "12", "RESETn": "13", "RFVDD": "14", "RFVSS": "15", "SUBG_I1": "16", "SUBG_I0": "17",
          "SUBG_O1": "18", "SUBG_O0": "19", "PAVDD": "20", "VREGVDD": "38", "VREGVSS": "39", "AVDD": "41", "IOVDD": "42", "EP": "49"}
ESP_GPIO_PIN = {0: "27", 3: "15", 10: "18", 11: "19", 12: "20", 13: "21", 14: "22", 19: "13", 20: "14", 35: "28", 36: "29", 37: "30",
                38: "31", 39: "32", 40: "33", 45: "26", 46: "16", 48: "25"}
ESP_3V3, ESP_GND = "2", ("1", "40", "41")
# (signal, FG23 port, ESP GPIO, driver)  [FW] + [MAP]
LINK = [("SCLK", "PC05", 12, "FG"), ("MOSI", "PC00", 11, "FG"), ("CS", "PA07", 10, "FG"), ("CMD", "PA06", 13, "ESP"),
        ("RDY", "PD02", 14, "ESP"), ("SWDIO", "PA02", 38, None), ("SWCLK", "PA01", 39, None), ("SWO", "PA03", 48, None),
        ("RESETn", "RESETn", 40, None)]
FORBIDDEN_ESP = [3, 45, 46, 35, 36, 37]           # strapping (3, 45, 46), octal PSRAM (35-37)
# RF chain from the SMA centre pin to SUBG_I0 [spec doc + AN923 + DS-QPL + SWL]; each step: (kind, value fragment)
RF_CHAIN = [("SAW", "HDF434A1"), ("C", "100p"), ("LNA", "QPL9547"), ("C", "100p"), ("SAW", "HDF434A1"), ("R", "0R"), ("C", "220p"), ("L", "82n")]

# ---------------------------------------------------------------- netlist model
def load(path):
    t = kfp.parse(open(path).read()); comps = {}; nets = {}
    for c in kfp.findall(kfp.find(t, "components"), "comp"):
        ref = kfp.uq(kfp.find(c, "ref")[1]); val = kfp.uq(kfp.find(c, "value")[1])
        props = {kfp.uq(kfp.find(pp, "name")[1]) for pp in kfp.findall(c, "property")}     # KiCad 7: (property (name "dnp"))
        comps[ref] = dict(value=val, dnp="dnp" in props)
    for n in kfp.findall(kfp.find(t, "nets"), "net"):
        name = kfp.uq(kfp.find(n, "name")[1])
        nets[name] = {(kfp.uq(kfp.find(x, "ref")[1]), kfp.uq(kfp.find(x, "pin")[1])) for x in kfp.findall(n, "node")}
    return comps, nets
class Board:
    def __init__(self, comps, nets):
        self.c, self.n = comps, nets
        self.pin = {node: name for name, nodes in nets.items() for node in nodes}
    def net(self, ref, pin): return self.pin.get((ref, pin), "?")
    def nodes(self, net): return self.n.get(net, set())
    def nc(self, ref, pin): return self.net(ref, pin).startswith("unconnected-") or self.net(ref, pin) == "?"
    def other(self, ref, pin):   # the other pin of a two-pin part
        return self.net(ref, "2" if pin == "1" else "1")
    def fitted(self, ref): return ref in self.c and not self.c[ref]["dnp"]
    def series_r(self, net, maxohm=100):
        """two-pin fitted resistors <= maxohm on a net: [(ref, pin_on_net)]"""
        out = []
        for ref, pin in self.nodes(net):
            if ref.startswith("R") and pin in ("1", "2") and self.fitted(ref) and ohms(self.c[ref]["value"]) <= maxohm: out.append((ref, pin))
        return out
def ohms(v):
    v = v.split()[0].upper().replace("Ω", "")
    mult = {"R": 1, "K": 1e3, "M": 1e6}
    for k, m in mult.items():
        if k in v:
            a, _, b = v.partition(k); return float((a or "0") + "." + (b or "0")) * m
    try: return float(v)
    except ValueError: return 1e12

# ---------------------------------------------------------------- checks
def run(b):
    res = []
    def check(ok, what, detail=""): res.append(("PASS" if ok else "FAIL", what, detail))
    def warn(what, detail=""): res.append(("WARN", what, detail))
    # 1. FG23 <-> ESP link, through at most one series resistor (<= 100R), driver-side termination
    for sig, port, gpio, drv in LINK:
        fn = b.net(FG, FG_PIN[port]); en = b.net(ESP, ESP_GPIO_PIN[gpio])
        if fn == en: path, rref = True, None
        else:
            rs = [r for r, p in b.series_r(fn) if b.other(r, p) == en]
            path, rref = bool(rs), (rs[0] if rs else None)
        check(path and fn != "?" and not fn.startswith("unconnected"), f"link {sig}: FG23 {port} <-> ESP GPIO{gpio}", f"{fn} / {en}")
        if drv and path:
            near = fn if drv == "FG" else en          # source termination: the R sits on the driver's own pin net
            ok = rref is not None and len(b.nodes(near)) == 2
            check(ok, f"link {sig}: 22R source termination at the {drv} side", f"net {near} nodes {sorted(b.nodes(near))}")
    rdy = b.net(FG, FG_PIN["PD02"])
    check(any(r.startswith("R") and b.other(r, p) == "GND" and 4e3 < ohms(b.c[r]["value"]) < 1e5 for r, p in b.nodes(rdy)), "RDY pull-down at the FG23", rdy)
    rst = b.net(FG, FG_PIN["RESETn"])
    check(any(r.startswith("J_SWD") for r, _ in b.nodes(rst)), "RESETn on the SWD connector", rst)
    # 2. ESP strapping / PSRAM pins left alone; IO0 = boot button only
    for gp in FORBIDDEN_ESP: check(b.nc(ESP, ESP_GPIO_PIN[gp]), f"ESP GPIO{gp} unconnected (strapping / octal PSRAM)", b.net(ESP, ESP_GPIO_PIN[gp]))
    io0 = b.net(ESP, ESP_GPIO_PIN[0]); refs = {r for r, _ in b.nodes(io0)} - {ESP}
    check(all(r.startswith(("SW_BOOT", "R_BOOT")) for r in refs) and refs, "ESP GPIO0 only to the BOOT button + pull-up", str(sorted(refs)))
    # 3. USB
    j = "J_USB1"
    dp = b.net(ESP, ESP_GPIO_PIN[20]); dm = b.net(ESP, ESP_GPIO_PIN[19])
    check(b.net(j, "A6") == b.net(j, "B6") == dp, "USB D+ (A6/B6) -> ESP GPIO20", dp)
    check(b.net(j, "A7") == b.net(j, "B7") == dm, "USB D- (A7/B7) -> ESP GPIO19", dm)
    for cc in ("A5", "B5"):
        n = b.net(j, cc)
        check(any(r.startswith("R") and abs(ohms(b.c[r]["value"]) - 5100) < 200 and b.other(r, p) == "GND" for r, p in b.nodes(n)), f"USB {cc}: 5.1k Rd to GND", n)
    # 4. supplies: ESP on one LDO, FG23 + LNA + TCXO on another, both from VBUS
    esp_rail = b.net(ESP, ESP_3V3); fg_rail = b.net(FG, FG_PIN["VREGVDD"])
    check(fg_rail == b.net(FG, FG_PIN["AVDD"]) == b.net(FG, FG_PIN["IOVDD"]), "FG23 VREGVDD/AVDD/IOVDD on one rail", fg_rail)
    check(esp_rail != fg_rail, "ESP and RF section on separate 3.3 V rails", f"{esp_rail} / {fg_rail}")
    def ldo_out(net):
        return [r for r, p in b.nodes(net) if r.startswith("U_LDO") and p == "5"]
    check(bool(ldo_out(esp_rail)) and bool(ldo_out(fg_rail)), "each rail driven by an LDO output (pin 5)", f"{ldo_out(esp_rail)} / {ldo_out(fg_rail)}")
    vb = b.net(j, "A4")
    for r in ldo_out(esp_rail) + ldo_out(fg_rail):
        vin = b.net(r, "1"); fb = [x for x, p in b.nodes(vin) if x.startswith("FB") and b.other(x, p) == vb]
        check(bool(fb), f"{r} input from VBUS through a ferrite", vin)
    for gp in ("RFVSS", "VREGVSS", "EP"): check(b.net(FG, FG_PIN[gp]) == "GND", f"FG23 {gp} = GND")
    for p in ESP_GND: check(b.net(ESP, p) == "GND", f"ESP pin {p} = GND")
    # 5. radio: RX only, TCXO
    for p in ("SUBG_I1", "SUBG_O0", "SUBG_O1", "HFXTAL_O"): check(b.nc(FG, FG_PIN[p]), f"FG23 {p} unconnected (RX only / external clock)", b.net(FG, FG_PIN[p]))
    hx = b.net(FG, FG_PIN["HFXTAL_I"])
    cap = [(r, p) for r, p in b.nodes(hx) if r.startswith("C") and b.fitted(r)]
    check(any(b.net("Y_TCXO1", "3") == b.other(r, p) for r, p in cap), "HFXTAL_I <- coupling C <- TCXO OUT", hx)
    check(b.net("Y_TCXO1", "2") == "GND", "TCXO pin 2 = GND")
    # 6. LNA: bias, shutdown, supply
    sd = b.net(LNA, "6")
    check(sd == b.net(FG, FG_PIN["PB00"]), "QPL9547 shut-down <- FG23 PB00", sd)
    check(any(r.startswith("R") and b.other(r, p) == "GND" for r, p in b.nodes(sd)), "QPL9547 shut-down pulled low (LNA on by default)", sd)
    vbias = b.net(LNA, "1")
    br = [r for r, p in b.nodes(vbias) if r.startswith("R") and 3000 < ohms(b.c[r]["value"]) < 3700]
    check(bool(br), "QPL9547 Vbias via ~3.3k", vbias)
    out = b.net(LNA, "7")
    chk = [r for r, p in b.nodes(out) if r.startswith("L") and b.fitted(r)]
    check(bool(chk) and b.other(chk[0], [p for r, p in b.nodes(out) if r == chk[0]][0]) == (b.other(br[0], [p for r, p in b.nodes(vbias) if r == br[0]][0]) if br else None),
          "QPL9547 VDD fed through an RF choke into RF Out, same supply as the bias resistor", out)
    check(b.net(LNA, "9") == "GND", "QPL9547 paddle = GND")
    # 7. RF chain order SMA -> SUBG_I0 (fitted parts only)
    net = b.net("J_ANT1", "1"); seen = []; ok = True
    target = b.net(FG, FG_PIN["SUBG_I0"])
    for kind, frag in RF_CHAIN:
        cand = []
        for r, p in b.nodes(net):
            if not b.fitted(r) or r in [s[0] for s in seen]: continue
            if kind == "SAW" and r.startswith("SAW") and p == "2": cand.append((r, b.net(r, "5")))
            elif kind == "LNA" and r == LNA and p == "2": cand.append((r, b.net(r, "7")))
            elif kind in "RCL" and r.startswith(kind) and not r.startswith(("RF", "LNA")) and p in ("1", "2") and frag in b.c[r]["value"] and b.other(r, p) != "GND":
                cand.append((r, b.other(r, p)))
        if len(cand) != 1: ok = False; seen.append((f"{kind}:{frag}?", f"{len(cand)} candidates on {net}")); break
        seen.append(cand[0]); net = cand[0][1]
    check(ok and net == target, "RF chain SMA -> SAW1 -> C -> QPL9547 -> C -> SAW2 -> pad -> C -> L 82n -> SUBG_I0", " > ".join(s[0] for s in seen) + f" > {net}")
    # shunts on the RF path: only DNP slots, the RF ESD and the pad (DNP) - nothing fitted may load the chain
    rf_nets = {b.net("J_ANT1", "1")} | {s[1] for s in seen if isinstance(s[1], str)}
    for n in rf_nets:
        for r, p in b.nodes(n):
            if b.fitted(r) and r[0] in "RCL" and b.other(r, p) == "GND" and not r.startswith("D_ESD"):
                check(False, f"no fitted shunt to GND on RF net {n}", r)
    return res

def main(path):
    comps, nets = load(path); res = run(Board(comps, nets))
    for s, w, d in res:
        if s != "PASS" or "-v" in sys.argv: print(f"  {s:4} {w}" + (f"   [{d}]" if d else ""))
    np_ = sum(1 for r in res if r[0] == "PASS"); nw = sum(1 for r in res if r[0] == "WARN"); nf = sum(1 for r in res if r[0] == "FAIL")
    print(f"twin_check L1: {np_} pass, {nw} warn, {nf} FAIL")
    return 1 if nf else 0

# ---------------------------------------------------------------- self-test: injected faults must be caught
def selftest(path):
    comps, nets = load(path)
    base = run(Board(comps, nets)); assert not [r for r in base if r[0] == "FAIL"], "base must pass first"
    def mv(n, node, to):   # move one pin to another net
        n = copy.deepcopy(n)
        for k in n: n[k].discard(node)
        n.setdefault(to, set()).add(node); return n
    def swap(n, a, b_):
        na, nb = None, None
        for k, v in n.items():
            if a in v: na = k
            if b_ in v: nb = k
        return mv(mv(n, a, nb), b_, na)
    faults = {
        "SCLK/MOSI swapped at the ESP": swap(nets, (ESP, "20"), (ESP, "19")),
        "USB D+/D- swapped": swap(nets, (ESP, "14"), (ESP, "13")),
        "RX inductor on SUBG_I1": mv(mv(nets, ("L_RX1", "2"), "unconnected-x"), (FG, "16"), "SUBG_I0"),
        "LNA in/out reversed": swap(nets, (LNA, "2"), (LNA, "7")),
        "strapping pin IO45 used": mv(nets, (ESP, "26"), "LED_ST"),
        "RDY pull-down missing": mv(nets, ("R_RDYPD1", "2"), "unconnected-y"),
        "ESP on the RF rail": mv(nets, (ESP, "2"), nets and [k for k, v in nets.items() if (FG, "38") in v][0]),
        "TCXO output not coupled": mv(nets, ("Y_TCXO1", "3"), "unconnected-z"),
    }
    caught = 0
    for name, n in faults.items():
        r = run(Board(comps, n)); f = [x for x in r if x[0] == "FAIL"]
        print(f"  {'caught' if f else 'MISSED'}  {name}" + (f"  -> {f[0][1]}" if f else "")); caught += bool(f)
    print(f"twin L1 self-test: {caught}/{len(faults)}")
    return 0 if caught == len(faults) else 3

if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    p = args[0] if args else os.path.join(HERE, "..", "kicad", "netlist.net")
    sys.exit(selftest(p) if "--selftest" in sys.argv else main(p))
