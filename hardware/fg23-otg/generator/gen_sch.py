#!/usr/bin/env python3
"""SatHunter FG23 OTG rev A - schematic in the house style (as ionos-maximus-rpi-sdr gen_sch.py):
1.27 mm grid, wired circuits with real KiCad symbols, supplies as upright power symbols, signals between
blocks as global labels, framed functional blocks with a title and a note, A2 with a filled title block.
Connectivity comes only from design.PARTS; check_netlist.py proves the drawing equals it.
Run: python3 gen_sch.py   (writes ../kicad/)
"""
import os, sys, uuid, json, copy, shutil
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import kfp, design
PARTS, R, fp_lib = design.PARTS, design.R, design.fp_lib
OUT = os.environ.get("SH_OUT") or os.path.join(HERE, "..", "kicad")
PRJ = "sathunter_fg23"; LIB = "SH"
NS = uuid.UUID("5a7b6e2e-3f1c-4e0a-9a51-53617448756e")
ROOT = str(uuid.uuid5(NS, "root"))
G = 1.27
def u(n): return round(n * 2.54, 4)
def g(v): return ("%.2f" % (round(v / G) * G)).rstrip("0").rstrip(".")
_ctr = [0]
def uid(*k):
    _ctr[0] += 1
    return str(uuid.uuid5(NS, "/".join(map(str, k)) + "#%d" % _ctr[0]))

# ============================================================================ symbols (KiCad 7 libraries + custom)
STD = "/usr/share/kicad/symbols"
_libs = {}
def stdsym(lib, name):
    if lib not in _libs:
        t = kfp.parse(open(os.path.join(STD, lib + ".kicad_sym")).read())
        _libs[lib] = {kfp.uq(e[1]): e for e in t if isinstance(e, list) and e and e[0] == "symbol"}
    s = copy.deepcopy(_libs[lib][name]); ext = kfp.find(s, "extends")
    if ext:                                   # flatten: parent body, child's own properties
        base = stdsym(lib, kfp.uq(ext[1])); pname = kfp.uq(base[1])
        props = {kfp.uq(p[1]): p for p in kfp.findall(s, "property")}
        for i, e in enumerate(base):
            if isinstance(e, list) and e and e[0] == "property" and kfp.uq(e[1]) in props: base[i] = props[kfp.uq(e[1])]
        s = base; s[1] = f'"{name}"'
        for sub in kfp.findall(s, "symbol"): sub[1] = '"' + kfp.uq(sub[1]).replace(pname, name, 1) + '"'
    return s
def rename(s, new):
    old = kfp.uq(s[1]); s = copy.deepcopy(s); s[1] = f'"{new}"'
    for sub in kfp.findall(s, "symbol"): sub[1] = '"' + kfp.uq(sub[1]).replace(old, new, 1) + '"'
    return s
def setprop(s, key, val):
    for p in kfp.findall(s, "property"):
        if kfp.uq(p[1]) == key: p[2] = f'"{val}"'
def power_sym(net, shape="+3V3"):
    s = rename(stdsym("power", shape), net); setprop(s, "Value", net); setprop(s, "Description", "Supply net " + net)
    def walk(n):
        for e in n:
            if isinstance(e, list) and e:
                if e[0] == "pin": kfp.find(e, "name")[1] = f'"{net}"'
                else: walk(e)
    walk(s); return s
def F(sz=1.27): return ["effects", ["font", ["size", str(sz), str(sz)]]]
def box_sym(name, ref, value, w, h, pins, fill=True):
    """pins: (num, name, x, y, angle) with (x, y) the pin END (connection point), length 2.54."""
    pp = []
    for num, nm, x, y, a in pins:
        hide = nm.startswith("~h")
        nm = nm[2:] if hide else nm
        pp.append(["pin", "passive", "line", ["at", str(x), str(y), str(a)], ["length", "2.54"]] + (["hide"] if hide else []) +
                  [["name", f'"{nm}"', F(1.0)], ["number", f'"{num}"', F(1.0)]])
    body = ["rectangle", ["start", str(-w), str(h)], ["end", str(w), str(-h)], ["stroke", ["width", "0.254"], ["type", "default"]],
            ["fill", ["type", "background" if fill else "none"]]]
    def prop(k, v, y, hide=False):
        e = F() + (["hide"] if hide else [])
        return ["property", f'"{k}"', f'"{v}"', ["at", "0", str(y), "0"], e]
    return ["symbol", f'"{name}"', ["pin_names", ["offset", "0.762"]], ["in_bom", "yes"], ["on_board", "yes"],
            prop("Reference", ref, h + 2.54), prop("Value", value, -h - 2.54), prop("Footprint", "", 0, True), prop("Datasheet", "", 0, True),
            ["symbol", f'"{name}_0_1"', body], ["symbol", f'"{name}_1_1"'] + pp]

def fg23_sym():
    L, Rx = -17.78, 17.78
    left = [(38, "VREGVDD", 27.94), (41, "AVDD", 25.4), (42, "IOVDD", 22.86), (40, "DVDD", 17.78), (37, "VREGSW", 15.24),
            (36, "DECOUPLE", 12.7), (14, "RFVDD", 10.16), (20, "PAVDD", 7.62), (13, "RESETn", 2.54), (11, "HFXTAL_I", -2.54), (12, "HFXTAL_O", -5.08)]
    unl = [(2, "PC1"), (3, "PC2"), (4, "PC3"), (5, "PC4"), (7, "PC6"), (8, "PC7"), (9, "PC8"), (10, "PC9"), (23, "PB1"), (22, "PB2"), (21, "PB3")]
    right = [(17, "SUBG_I0", 27.94), (16, "SUBG_I1", 25.4), (19, "SUBG_O0", 22.86), (18, "SUBG_O1", 20.32),
             (1, "PC0", 15.24), (6, "PC5", 12.7), (32, "PA7", 10.16), (31, "PA6", 5.08), (46, "PD2", 2.54), (24, "PB0", -2.54),
             (26, "PA1", -7.62), (27, "PA2", -10.16), (28, "PA3", -12.7)]
    unr = [(25, "PA0"), (29, "PA4"), (30, "PA5"), (33, "PA8"), (34, "PA9"), (35, "PA10"), (48, "PD0"), (47, "PD1"), (45, "PD3"), (44, "PD4"), (43, "PD5")]
    pins = [(n, nm, L, y, 0) for n, nm, y in left] + [(n, nm, L, -10.16 - 2.54 * i, 0) for i, (n, nm) in enumerate(unl)]
    pins += [(n, nm, Rx, y, 180) for n, nm, y in right] + [(n, nm, Rx, -17.78 - 2.54 * i, 180) for i, (n, nm) in enumerate(unr)]
    pins += [(15, "RFVSS", 0, -48.26, 90), (39, "~hVREGVSS", 0, -48.26, 90), (49, "~hEPAD", 0, -48.26, 90)]
    return box_sym("EFR32FG23", "U", "EFR32FG23", 15.24, 45.72, pins)
def saw_sym():
    return box_sym("SAW", "SAW", "SAW", 7.62, 3.81, [(2, "IN", -10.16, 0, 0), (5, "OUT", 10.16, 0, 180), (1, "GND", 0, -6.35, 90),
                   (3, "~hGND", 0, -6.35, 90), (4, "~hGND", 0, -6.35, 90), (6, "~hGND", 0, -6.35, 90)])
def lna_sym():
    return box_sym("QPL9547", "U", "QPL9547", 10.16, 7.62, [(2, "RF_IN", -12.7, 0, 0), (7, "RF_OUT", 12.7, 0, 180), (1, "VBIAS", -5.08, 10.16, 270),
                   (6, "SHDN", -5.08, -10.16, 90), (3, "GND", 5.08, -10.16, 90), (4, "~hNC", 5.08, -10.16, 90), (5, "~hNC", 5.08, -10.16, 90),
                   (8, "~hNC", 5.08, -10.16, 90), (9, "~hEP", 5.08, -10.16, 90)])
def tcxo_sym():
    return box_sym("TCXO", "Y", "TCXO", 5.08, 5.08, [(4, "VCC", 0, 7.62, 270), (2, "GND", 0, -7.62, 90), (3, "OUT", 7.62, 0, 180), (1, "VC", -7.62, 0, 0)])

SYMLIB = {
 "R": stdsym("Device", "R"), "C": stdsym("Device", "C"), "L": stdsym("Device", "L"), "FB": rename(stdsym("Device", "FerriteBead"), "FB"),
 "LED": stdsym("Device", "LED"), "D_TVS": stdsym("Device", "D_TVS"), "SW_Push": stdsym("Switch", "SW_Push"),
 "TestPoint": stdsym("Connector", "TestPoint"), "Conn_Coaxial": stdsym("Connector", "Conn_Coaxial"),
 "TC2030": rename(stdsym("Connector", "Conn_ARM_SWD_TagConnect_TC2030"), "TC2030"),
 "USB_C": rename(stdsym("Connector", "USB_C_Receptacle_USB2.0_16P"), "USB_C"),
 "ESP32-S3-WROOM-1": stdsym("RF_Module", "ESP32-S3-WROOM-1"),
 "LP5907MFX-3.3": stdsym("Regulator_Linear", "LP5907MFX-3.3"), "AP2112K-3.3": stdsym("Regulator_Linear", "AP2112K-3.3"),
 "USBLC6-2SC6": stdsym("Power_Protection", "USBLC6-2SC6"),
 "EFR32FG23": fg23_sym(), "SAW": saw_sym(), "QPL9547": lna_sym(), "TCXO": tcxo_sym(),
 "GND": stdsym("power", "GND"), "PWR_FLAG": stdsym("power", "PWR_FLAG"),
}
POWER = ["V5", "V3D", "V3RF", "VBUS", "V_LNA", "V_TCXO"]
for n in POWER: SYMLIB[n] = power_sym(n)
SYM_OF = {}
for ref, p in PARTS.items():
    fp = p["fp"]
    SYM_OF[ref] = {"R0402": "R", "R0603": "R", "C0402": "C", "C0603": "C", "C0805": "C", "L0402": "L", "L0603": "L", "L0805": "L", "L_2016": "L",
                   "D0402": "D_TVS", "LED_0603": "LED", "SW_SMD": "SW_Push", "TP": "TestPoint", "SMA": "Conn_Coaxial", "TC2030": "TC2030",
                   "USBC": "USB_C", "WROOM1": "ESP32-S3-WROOM-1", "QFN48": "EFR32FG23", "SAW6": "SAW", "DFN8_2x2": "QPL9547", "OSC2520": "TCXO"}.get(fp)
    if ref.startswith("FB_"): SYM_OF[ref] = "FB"
    if ref.startswith("U_LDOD"): SYM_OF[ref] = "AP2112K-3.3"
    if ref.startswith("U_LDORF"): SYM_OF[ref] = "LP5907MFX-3.3"
    if ref.startswith("U_ESD"): SYM_OF[ref] = "USBLC6-2SC6"
    assert SYM_OF[ref], ref
def sym_pins(name):
    out = {}
    def walk(n):
        for e in n:
            if isinstance(e, list) and e:
                if e[0] == "pin":
                    at = kfp.find(e, "at"); out[kfp.uq(kfp.find(e, "number")[1])] = (float(at[1]), float(at[2]))
                else: walk(e)
    walk(SYMLIB[name]); return out
for ref, name in SYM_OF.items():                      # every design pin exists on its symbol and vice versa
    assert set(sym_pins(name)) == set(PARTS[ref]["pins"]), (ref, name, set(sym_pins(name)) ^ set(PARTS[ref]["pins"]))

SHORT = {"J_ANT": "SMA edge", "D_ESD": "RF ESD 0.3pF", "SW_RST": "RESET", "SW_BOOT": "BOOT", "D_LST": "green", "D_LPW": "red",
         "J_USB": "USB-C GCT USB4105", "J_SWD": "TC2030-IDC-NL", "Y_TCXO": "39.000 MHz TCXO", "U_FG23": "EFR32FG23 QFN48",
         "U_ESP": "ESP32-S3-WROOM-1-N16R8", "U_LDOD": "AP2112K-3.3", "U_LDORF": "LP5907MFX-3.3", "U_ESD": "USBLC6-2SC6"}
def short_value(ref):
    base = ref[:-1] if ref not in SHORT and ref[:-1] in SHORT else ref
    if base in SHORT: return SHORT[base]
    v = PARTS[ref]["value"]
    if ref[0] in "RCLF" and SYM_OF[ref] in ("R", "C", "L", "FB"):
        t = v.split()
        return t[0] if t[0][0].isdigit() or t[0] == "NM" else t[-1]
    return v

# ============================================================================ sheet
class Sheet:
    def __init__(self):
        self.items = []; self.pos = {}; self.npwr = 0; self.nflg = 0; self.placed = set()
    def wire(self, *pts):
        for a, b in zip(pts, pts[1:]):
            if a != b: self.items.append(["wire", ["pts", ["xy", g(a[0]), g(a[1])], ["xy", g(b[0]), g(b[1])]], ["stroke", ["width", "0"], ["type", "default"]], ["uuid", uid("w")]])
    def junction(self, p): self.items.append(["junction", ["at", g(p[0]), g(p[1])], ["diameter", "0"], ["color", "0", "0", "0", "0"], ["uuid", uid("j")]])
    def nc(self, p): self.items.append(["no_connect", ["at", g(p[0]), g(p[1])], ["uuid", uid("nc")]])
    def label(self, net, p, ang=0):
        just = {0: "left bottom", 180: "right bottom", 90: "left bottom", 270: "right bottom"}[ang]
        self.items.append(["label", f'"{net}"', ["at", g(p[0]), g(p[1]), str(ang)], ["fields_autoplaced"], F(1.0) + [["justify"] + just.split()], ["uuid", uid("l")]])
    def glabel(self, net, p, right=True):
        self.items.append(["global_label", f'"{net}"', ["shape", "passive"], ["at", g(p[0]), g(p[1]), "0" if right else "180"], ["fields_autoplaced"],
                           F() + [["justify", "left" if right else "right"]], ["uuid", uid("gl")],
                           ["property", '"Intersheetrefs"', '"${INTERSHEET_REFS}"', ["at", g(p[0]), g(p[1]), "0"], F() + ["hide"]]])
    def text(self, s, p, size=1.27, bold=False):
        for i, ln in enumerate(s.split("\n")):
            e = ["effects", ["font", ["size", str(size), str(size)]] + ([["thickness", "0.5"], "bold"] if bold else []), ["justify", "left", "bottom"]]
            self.items.append(["text", f'"{ln}"', ["at", g(p[0]), g(p[1] + 2.54 * i * size / 1.27), "0"], e, ["uuid", uid("t")]])
    def block(self, title, note, x1, y1, x2, y2):
        self.items.append(["rectangle", ["start", g(x1), g(y1)], ["end", g(x2), g(y2)], ["stroke", ["width", "0.254"], ["type", "dash"]], ["fill", ["type", "none"]], ["uuid", uid("b")]])
        self.text(title, (x1 + 2.54, y1 + 6.35), 2.5, bold=True)
        if note: self.text(note, (x1 + 2.54, y1 + 10.16), 1.27)
    # ---- symbols: rot = KiCad angle (CCW), mirror = 'y' flips left/right
    @staticmethod
    def xf(px, py, rot, mirror):
        if mirror: px = -px
        if rot == 90: px, py = -py, px
        elif rot == 180: px, py = -px, -py
        elif rot == 270: px, py = py, -px
        return px, py
    def pin(self, ref, num):
        if ref not in self.pos: ref = R(ref)
        x, y, lib, rot, mir = self.pos[ref]
        px, py = sym_pins(lib)[num]; px, py = self.xf(px, py, rot, mir)
        return (round(x + px, 4), round(y - py, 4))
    def symbol(self, ref, lib, value, fp, x, y, rot=0, mirror=False, fields=None, dnp=False, power=False, extra=None):
        self.pos[ref] = (x, y, lib, rot, mirror)
        fa = str((360 - rot) % 360) if not power else "0"
        o = ["symbol", ["lib_id", f'"{LIB}:{lib}"'], ["at", g(x), g(y), str(rot)]] + ([["mirror", "y"]] if mirror else []) + \
            [["unit", "1"], ["in_bom", "no" if power else "yes"], ["on_board", "no" if power else "yes"], ["dnp", "yes" if dnp else "no"], ["uuid", uid("s", ref)]]
        def P(k, v, px, py, hide=False, just=None):
            e = F() + ([["justify"] + just.split()] if just else []) + (["hide"] if hide else [])
            return ["property", f'"{k}"', f'"{v}"', ["at", g(px), g(py), fa], e]
        if power:
            o.append(P("Reference", ref, x, y, hide=True))
            o.append(P("Value", value, x, y + 3.81 if lib == "GND" else y - 3.81, hide=lib == "PWR_FLAG"))
        else:
            (rx, ry, rj), (vx, vy, vj) = fields
            o.append(P("Reference", ref, x + rx, y + ry, just=rj)); o.append(P("Value", value, x + vx, y + vy, just=vj, hide=lib == "TestPoint"))
        o.append(P("Footprint", fp, x, y, hide=True)); o.append(P("Datasheet", "~", x, y, hide=True))
        for k, v in (extra or {}).items(): o.append(P(k, v, x, y, hide=True))
        for p in sym_pins(lib): o.append(["pin", f'"{p}"', ["uuid", uid("p", ref, p)]])
        o.append(["instances", ["project", f'"{PRJ}"', ["path", f'"/{ROOT}"', ["reference", f'"{ref}"'], ["unit", "1"]]]])
        self.items.append(o)
    def part(self, ref, x, y, rot=0, mirror=False, fields=None):
        ref = R(ref); p = PARTS[ref]; lib = SYM_OF[ref]; self.placed.add(ref)
        self.symbol(ref, lib, short_value(ref), fp_lib(p["fp"]), x, y, rot, mirror, fields or F_SER, dnp=p["dnp"],
                    extra={"MPN / note": p["value"] + ("; " + p["desc"] if p["desc"] else "")})
        return ref
    def power(self, net, p):
        self.npwr += 1; self.symbol("#PWR%03d" % self.npwr, net, net, "", p[0], p[1], power=True)
    def flag(self, p):
        self.nflg += 1; self.symbol("#FLG%02d" % self.nflg, "PWR_FLAG", "PWR_FLAG", "", p[0], p[1], power=True)

# field presets ((ref dx, dy, justify), (value dx, dy, justify))
F_SER = ((0, -2.54, None), (0, 2.54, None))              # horizontal two-pin
F_VER = ((2.54, -1.27, "left"), (2.54, 1.27, "left"))     # vertical two-pin
F_VERL = ((-2.54, -1.27, "right"), (-2.54, 1.27, "right"))

sh = Sheet()
# ---------------------------------------------------------------- building blocks
def series(ref, a, flip=False):
    """horizontal two-pin part, pin 1 on the left at point a (flip: pin 1 on the right, i.e. a is pin 2)"""
    lib = SYM_OF[R(ref)]; half = 5.08 if lib == "SW_Push" else 3.81
    x, y = a[0] + half, a[1]
    if lib in ("R", "C", "L", "FB"): sh.part(ref, x, y, 270 if flip else 90, fields=F_SER)
    else: sh.part(ref, x, y, 0, mirror=flip, fields=F_SER)
    p1, p2 = sh.pin(R(ref), "1"), sh.pin(R(ref), "2")
    return p1 if flip else p2
def vertical(ref, top, fields=F_VER):
    """vertical two-pin part, pin 1 at `top`; returns pin 2"""
    lib = SYM_OF[R(ref)]; half = 5.08 if lib == "SW_Push" else 3.81
    x, y = top[0], top[1] + half
    if lib in ("R", "C", "L", "FB"): sh.part(ref, x, y, 0, fields=fields)
    else: sh.part(ref, x, y, 270, fields=fields)
    assert abs(sh.pin(R(ref), "1")[1] - top[1]) < 0.01, ref
    return sh.pin(R(ref), "2")
def shunt(ref, node, fields=F_VER, junction=True):
    assert PARTS[R(ref)]["pins"]["2"] == "GND", ref
    top = (node[0], node[1] + 2.54); sh.wire(node, top); b = vertical(ref, top, fields); sh.power("GND", b)
    if junction: sh.junction(node)
def pullup(ref, node, rail, fields=F_VER):
    assert PARTS[R(ref)]["pins"]["1"] == rail, ref
    p2 = (node[0], node[1] - 2.54); top = (node[0], p2[1] - 7.62)
    vertical(ref, top, fields); sh.wire(p2, node); sh.power(rail, top); sh.junction(node)
def decap_row(rail, refs, x0, y, step=12.7):
    sh.power(rail, (x0, y)); x = x0
    for r in refs:
        x += step; shunt(r, (x, y), junction=r != refs[-1])
    sh.wire((x0, y), (x, y))
def stub(p, dx, dy=0):
    e = (p[0] + dx, p[1] + dy); sh.wire(p, e); return e

# ============================================================================ A: RF front end
YA = u(31)
sh.block("RF front end, 430-440 MHz", "SMA - RF ESD - SAW1 - QPL9547 (~25 dB) - SAW2 - pi-pad option - RX match - FG23 SUBG_I0.  "
         "Gain to the FG23 ~21 dB, system NF ~2.6 dB (0.6 dB with SAW1 bypassed).  Shield can over SAW1 ... FG23 + TCXO.", u(6), u(6), u(128), u(50))
sh.part("J_ANT", u(12), YA, 0, mirror=True, fields=((0, -5.08, None), (-7.62, 2.54, "right")))
sh.power("GND", sh.pin("J_ANT1", "2"))
a = sh.pin("J_ANT1", "1"); n0 = (u(17), YA); sh.wire(a, n0); sh.label("ANT", (u(15), YA))
d = (n0[0], n0[1] + 2.54); sh.wire(n0, d); sh.part("D_ESD", d[0], d[1] + 3.81, 270, fields=F_VER); sh.power("GND", sh.pin("D_ESD1", "2")); sh.junction(n0)
n1 = (u(20), YA); sh.wire(n0, n1); sh.junction(n1)
sh.part("SAW1", u(27), YA, fields=((0, -8.89, None), (0, -6.35, None)))
sh.wire(n1, sh.pin("SAW1", "2")); sh.power("GND", sh.pin("SAW1", "1"))
n2 = (u(33), YA); sh.wire(sh.pin("SAW1", "5"), n2); sh.junction(n2); sh.label("RF_S1O", (u(31), YA))
byp_y = YA - u(6); sh.wire(n1, (n1[0], byp_y)); p = series("R_BYP", (u(24.5), byp_y)); sh.wire((n1[0], byp_y), (u(24.5), byp_y)); sh.wire(p, (n2[0], byp_y), n2)
sh.text("SAW1 bypass:\nfit R_BYP1, remove SAW1", (u(22), byp_y - u(2.5)), 1.0)
p = series("C_LNI", (u(35), YA)); sh.wire(n2, (u(35), YA))
sh.part("U_LNA", u(44), YA, fields=((5.08, -12.7, "left"), (7.62, 13.97, "left")))
sh.wire(p, sh.pin("U_LNA1", "2")); sh.label("RF_LNI", (u(38), YA))
vb = sh.pin("U_LNA1", "1"); nb = (vb[0], YA - u(7)); sh.wire(vb, nb); sh.label("LNA_VB", (vb[0], YA - u(4.5)), 90)
pullup("R_BIAS", nb, "V_LNA")
cb = (u(38), nb[1]); sh.wire(nb, cb); shunt("C_BIAS", cb, fields=F_VERL, junction=False)
sd = sh.pin("U_LNA1", "6"); ns = (sd[0], YA + u(7)); sh.wire(sd, ns); shunt("R_SDPD", ns)
gl = (u(37), ns[1]); sh.wire(ns, gl); sh.glabel("LNA_SD", gl, right=False)
sh.power("GND", sh.pin("U_LNA1", "3"))
n3 = (u(51), YA); sh.wire(sh.pin("U_LNA1", "7"), n3); sh.label("RF_LNO", (u(50), YA))
pullup("L_CHK", n3, "V_LNA")
p = series("C_LNO", (u(53), YA)); sh.wire(n3, (u(53), YA))
sh.part("SAW2", u(62), YA, fields=((0, -8.89, None), (0, -6.35, None)))
sh.wire(p, sh.pin("SAW2", "2")); sh.label("RF_S2I", (u(56.5), YA)); sh.power("GND", sh.pin("SAW2", "1"))
n4 = (u(68), YA); sh.wire(sh.pin("SAW2", "5"), n4); sh.label("RF_S2O", (u(66), YA)); shunt("R_PD1", n4)
p = series("R_PDS", (u(70), YA)); sh.wire(n4, (u(70), YA))
n5 = (u(75), YA); sh.wire(p, n5); sh.label("RF_PD", (u(73), YA)); shunt("R_PD2", n5)
p = series("C_RXDC", (u(77), YA)); sh.wire(n5, (u(77), YA))
n6 = (u(82), YA); sh.wire(p, n6); sh.label("RF_RXN", (u(80), YA)); shunt("C_RXN", n6)
p = series("L_RX", (u(84), YA)); sh.wire(n6, (u(84), YA))
n7 = (u(89), YA); sh.wire(p, n7); shunt("C_RXS", n7)
e = (u(92), YA); sh.wire(n7, e); sh.glabel("SUBG_I0", e)
sh.text("pi-pad: 0R fitted = no pad.\n3 dB: R_PDS1 18R, R_PD1/R_PD2 300R.", (u(68), YA + u(9)), 1.0)
sh.text("RX match per AN923.2 Tab. 4.1 / BRD4265B:\n82 nH series, shunt C not fitted at 434 MHz.", (u(85), YA + u(9)), 1.0)
sh.text("QPL9547: Shut Down high = off.\nBias 3.32k (QPL9547EVB-01 R4).", (u(22), YA + u(11)), 1.0)
# LNA supply
ys = u(17); sh.power("V3RF", (u(66), ys)); p = series("FB_LNA", (u(68), ys)); sh.wire((u(66), ys), (u(68), ys))
nv = (u(74), ys); sh.wire(p, nv); sh.power("V_LNA", nv); sh.junction(nv)
shunt("C_LNA1", (u(77), ys)); shunt("C_LNA2", (u(82), ys), junction=False); sh.wire(nv, (u(82), ys))
sh.text("LNA supply", (u(66), ys - u(3)), 1.27)

# ============================================================================ B: EFR32FG23
XF, YF = u(56), u(95)
sh.block("EFR32FG23 radio (RX only)", "Link pins = ionos firmware: PC00 MOSI, PC05 SCLK, PA07 CS, PA06 CMD (rx), PD02 RDY (driven by the ESP32), PA03 SWO.  "
         "22R source termination at the driver.  Supply values as fg23-cw-trainer A5 / BRD4265B.", u(6), u(54), u(128), u(132))
sh.part("U_FG23", XF, YF, fields=((0, -49.53, None), (0, 55.88, None)))
# V3RF bus for VREGVDD / AVDD / IOVDD
bus = [stub(sh.pin("U_FG23", n), -5.08) for n in ("38", "41", "42")]
sh.wire(bus[0], bus[2]); sh.junction(bus[1]); top = (bus[0][0], bus[0][1] - 2.54); sh.wire(bus[0], top); sh.power("V3RF", top); sh.junction(bus[0])
for n, net in (("40", "VDCDC"), ("37", "VREGSW"), ("36", "DECOUPLE"), ("14", "RFVDD"), ("20", "PAVDD"), ("11", "HFXI")):
    e = stub(sh.pin("U_FG23", n), -7.62); sh.label(net, e, 180)
e = stub(sh.pin("U_FG23", "13"), -7.62); sh.glabel("RESETn", e, right=False)
for n, net in PARTS[R("U_FG23")]["pins"].items():
    if net == "": sh.nc(sh.pin("U_FG23", n))
sh.power("GND", sh.pin("U_FG23", "15"))
# right side: FG23-driven link lines through 22R, the rest to global labels
for n, net in (("1", "MOSI_F"), ("6", "SCLK_F"), ("32", "CS_F")):
    e = stub(sh.pin("U_FG23", n), 5.08); sh.label(net, e)
def chain(loc, ref, glob, x, y, right=True):
    """local net -> series part -> global label (one row)"""
    s0 = (x, y); sh.label(loc, s0, 180); p = series(ref, (x + 2.54, y)); sh.wire(s0, (x + 2.54, y)); e = stub(p, 5.08); sh.glabel(glob, e, right=right)
for n, net in (("17", "SUBG_I0"), ("31", "FG_CMD"), ("46", "FG_RDY"), ("24", "LNA_SD"), ("26", "SWCLK"), ("27", "SWDIO"), ("28", "SWO")):
    e = stub(sh.pin("U_FG23", n), 5.08); sh.glabel(net, e)
# supply circuits, left of the chip
X0 = u(9)
sh.text("Decoupling at VREGVDD / AVDD / IOVDD", (X0, u(62)), 1.27)
decap_row("V3RF", ["C_VREG1", "C_VREG2", "C_AVDD1", "C_AVDD2", "C_IOVDD1", "C_IOVDD2"], X0, u(66))
sh.text("DC-DC, RFVDD, PAVDD (BRD4265B: PAVDD from the DC-DC, R211 fitted)", (X0, u(77)), 1.27)
y1 = u(81); s = (X0, y1); sh.label("VREGSW", s, 180); p = series("L_DCDC", (X0 + u(1), y1)); sh.wire(s, (X0 + u(1), y1))
nd = (X0 + u(6), y1); sh.wire(p, nd); up = (nd[0], nd[1] - 2.54); sh.wire(nd, up); sh.label("VDCDC", up, 90); sh.junction(nd)
shunt("C_DCDC1", (X0 + u(9), y1)); shunt("C_DCDC2", (X0 + u(14), y1))
p = series("FB_RF", (X0 + u(16), y1)); sh.wire(nd, (X0 + u(16), y1))
nr = (X0 + u(21), y1); sh.wire(p, nr); shunt("C_RF1", nr); shunt("C_RF2", (X0 + u(26), y1))
e = (X0 + u(29), y1); sh.wire(nr, e); sh.label("RFVDD", e)
y2 = u(93); s = (X0, y2); sh.label("VDCDC", s, 180); p = series("R_PA", (X0 + u(1), y2)); sh.wire(s, (X0 + u(1), y2))
npf = (X0 + u(6), y2); sh.wire(p, npf); up = (npf[0], npf[1] - 2.54); sh.wire(npf, up); sh.label("PAVDD_F", up, 90); sh.junction(npf)
shunt("C_PA1", (X0 + u(9), y2)); shunt("C_PA2", (X0 + u(14), y2))
p = series("FB_PA", (X0 + u(16), y2)); sh.wire(npf, (X0 + u(16), y2)); e = (X0 + u(22), y2); sh.wire(p, e); sh.label("PAVDD", e)
y3 = u(104); s = (X0, y3); sh.label("DECOUPLE", s, 180); nd2 = (X0 + u(4), y3); sh.wire(s, nd2); shunt("C_DEC", nd2, junction=False)
s = (X0 + u(10), y3); sh.glabel("RESETn", s, right=False); nr2 = (X0 + u(14), y3); sh.wire(s, nr2); shunt("C_RST", nr2, junction=False)
# TCXO
sh.text("39 MHz TCXO (clipped sine) -> 1 nF -> HFXTAL_I; HFXTAL_O open.  Vc parts only for a VCTCXO (not fitted).", (X0, u(114)), 1.27)
yt = u(118); sh.power("V3RF", (X0, yt)); p = series("FB_TCX", (X0 + u(1), yt)); sh.wire((X0, yt), (X0 + u(1), yt))
nt = (X0 + u(6), yt); sh.wire(p, nt); sh.power("V_TCXO", nt); sh.junction(nt)
shunt("C_TC1", (X0 + u(9), yt)); shunt("C_TC2", (X0 + u(14), yt), junction=False); sh.wire(nt, (X0 + u(14), yt))
sh.part("Y_TCXO", X0 + u(32), yt + u(4), fields=((7.62, -6.35, "left"), (7.62, 6.35, "left")))
sh.power("V_TCXO", sh.pin("Y_TCXO", "4")); sh.power("GND", sh.pin("Y_TCXO", "2"))
o = sh.pin("Y_TCXO", "3"); b = stub(o, 12.7); sh.label("TCXO_OUT", (o[0] + 1.27, o[1])); p = series("C_TCX", b); e = stub(p, 5.08); sh.label("HFXI", e)
vc = sh.pin("Y_TCXO", "1"); nvc = (vc[0] - u(5), vc[1]); sh.wire(vc, nvc); sh.label("TCXO_VC", (vc[0] - u(4), vc[1]))
pullup("R_VC1", nvc, "V_TCXO"); shunt("R_VC2", nvc)
nc2 = (nvc[0] - u(5), nvc[1]); sh.wire(nvc, nc2); shunt("C_VC", nc2, junction=False)
# pull-up / pull-down on the link, near the FG23
sh.text("FG23-driven link lines: 22R at the FG23", (u(84), u(62)), 1.27)
for i, (loc, r, glob) in enumerate((("MOSI_F", "R_MOSI", "FG_MOSI"), ("SCLK_F", "R_SCLK", "FG_SCLK"), ("CS_F", "R_CS", "FG_CS"))):
    chain(loc, r, glob, u(90), u(66) + u(3) * i)
sh.text("Pull-up / pull-down", (u(84), u(77)), 1.27)
sh.power("V3RF", (u(88), u(80))); p = vertical("R_CSPU", (u(88), u(80))); e = stub(p, 0, 2.54); f = stub(e, 5.08); sh.glabel("FG_CS", f)
g0 = (u(100), u(83)); sh.glabel("FG_RDY", g0, right=False); nrd = (u(104), u(83)); sh.wire(g0, nrd); shunt("R_RDYPD", nrd, junction=False)
sh.text("CS idle high while the FG23 is in reset;\nRDY low (= ESP not ready) while the ESP boots.", (u(84), u(93)), 1.0)
# SWD
sh.text("FG23 SWD: Tag-Connect TC2030 (same pinout as fg23-cw-trainer);\nthe ESP32 also bit-bangs SWD on these nets (firmware update from the phone).", (u(84), u(99)), 1.0)
sh.part("J_SWD", u(90), u(114), fields=((2.54, -12.7, "left"), (2.54, 13.97, "left")))
sh.power("V3RF", sh.pin("J_SWD1", "1")); sh.power("GND", sh.pin("J_SWD1", "5"))
for n, net in (("2", "SWDIO"), ("3", "RESETn"), ("4", "SWCLK"), ("6", "SWO")):
    e = stub(sh.pin("J_SWD1", n), 5.08); sh.glabel(net, e)

# ============================================================================ E: ESP32-S3
XE, YE = u(186), u(46)
sh.block("ESP32-S3-WROOM-1-N16R8", "Same module as the ionos-mini DevKit: SPI2 IO_MUX slave for the FG23 (IO10-12), TinyUSB CDC to the phone, "
         "SWD bit-bang to the FG23.  IO3 / IO45 / IO46 (strapping) and IO35-37 (octal PSRAM) unused.", u(136), u(6), u(228), u(88))
sh.part("U_ESP", XE, YE, fields=((5.08, -30.48, "left"), (5.08, 31.75, "left")))
sh.power("V3D", sh.pin("U_ESP", "2")); sh.power("GND", sh.pin("U_ESP", "1"))
for n, net in PARTS[R("U_ESP")]["pins"].items():
    if net == "": sh.nc(sh.pin("U_ESP", n))
e = stub(sh.pin("U_ESP", "3"), -5.08); sh.label("ESP_EN", e, 180)
e = stub(sh.pin("U_ESP", "27"), -5.08); sh.label("ESP_BOOT", e, 180)
for n, net in (("18", "FG_CS"), ("19", "FG_MOSI"), ("20", "FG_SCLK")):
    e = stub(sh.pin("U_ESP", n), -5.08); sh.glabel(net, e, right=False)
for n, loc in (("21", "CMD_E"), ("22", "RDY_E")):
    e = stub(sh.pin("U_ESP", n), -5.08); sh.label(loc, e, 180)
for n, net in (("13", "USB_DM"), ("14", "USB_DP"), ("31", "SWDIO"), ("32", "SWCLK"), ("25", "SWO")):
    e = stub(sh.pin("U_ESP", n), 5.08); sh.glabel(net, e)
for n, loc in (("33", "FRST_E"), ("23", "LED_ST")):
    e = stub(sh.pin("U_ESP", n), 5.08); sh.label(loc, e)
sh.text("ESP-driven link lines: 22R at the ESP32; status LED", (u(140), u(62)), 1.27)
for i, (loc, r, glob) in enumerate((("CMD_E", "R_CMD", "FG_CMD"), ("RDY_E", "R_RDY", "FG_RDY"), ("FRST_E", "R_FRST", "RESETn"))):
    chain(loc, r, glob, u(144), u(66) + u(3) * i)
ys2 = u(76); s0 = (u(144), ys2); sh.label("LED_ST", s0, 180); p = series("R_LST", (u(145), ys2)); sh.wire(s0, (u(145), ys2))
c = stub(p, 5.08); sh.label("LED_ST_A", (p[0] + 1.27, p[1])); p = series("D_LST", c, flip=True); e = stub(p, 2.54); sh.power("GND", e)
a = sh.pin("U_ESP", "37"); e = stub(a, u(4)); sh.label("U0TXD", (a[0] + 2.54, a[1])); sh.part("TP_TX", e[0], e[1], fields=((2.54, -3.81, "left"), (0, 0, None)))
a = sh.pin("U_ESP", "36"); e = stub(a, u(8)); sh.label("U0RXD", (a[0] + 2.54, a[1])); sh.part("TP_RX", e[0], e[1], fields=((2.54, -3.81, "left"), (0, 0, None)))
# EN / BOOT / decoupling / power LED
sh.text("Reset (EN RC) and download mode (IO0 low at reset)", (u(140), u(22)), 1.27)
ye = u(30); s = (u(140), ye); sh.label("ESP_EN", s, 180); ne = (u(143), ye); sh.wire(s, ne)
pullup("R_EN", ne, "V3D"); shunt("C_EN", ne, junction=False)
nsw = (u(147), ye); sh.wire(ne, nsw); shunt("SW_RST", nsw, junction=False)
s = (u(152), ye); sh.label("ESP_BOOT", s, 180); nbt = (u(155), ye); sh.wire(s, nbt); pullup("R_BOOT", nbt, "V3D"); shunt("SW_BOOT", nbt, junction=False)
sh.text("3V3 decoupling at the module", (u(140), u(40)), 1.27)
decap_row("V3D", ["C_ESP1", "C_ESP2"], u(140), u(44))
sh.text("Power LED", (u(158), u(40)), 1.27)
yl = u(44); sh.power("V3D", (u(158), yl)); p = series("R_LPW", (u(160), yl)); sh.wire((u(158), yl), (u(160), yl))
c = stub(p, 5.08); sh.label("LED_PW_A", (p[0] + 1.27, p[1])); p = series("D_LPW", c, flip=True); e = stub(p, 2.54); sh.power("GND", e)

# ============================================================================ F: USB-C and supplies
sh.block("USB-C (OTG device) and supplies", "Phone 5 V -> ferrite -> V5 -> AP2112K-3.3 (ESP32-S3, WiFi peaks) and LP5907-3.3 (clean RF rail: FG23, LNA, TCXO).  "
         "USB 2.0 sink: 5.1k Rd on CC1 / CC2.", u(136), u(94), u(228), u(144))
ysp = u(106); sh.power("VBUS", (u(140), ysp)); p = series("FB_VBUS", (u(142), ysp)); sh.wire((u(140), ysp), (u(142), ysp))
n5v = (u(148), ysp); sh.wire(p, n5v); sh.power("V5", n5v); sh.junction(n5v)
shunt("C_V5A", (u(151), ysp)); shunt("C_V5B", (u(156), ysp)); f = (u(160), ysp); sh.wire(n5v, f); sh.flag(f); sh.junction(f)
sh.part("TP_5V", u(162), ysp, fields=((2.54, -3.81, "left"), (0, 0, None))); sh.wire(f, (u(162), ysp))
def ldo(ref, cin, cout, rail, y, x0, tp=None):
    sh.power("V5", (x0, y)); nin = (x0 + u(2), y); sh.wire((x0, y), nin); shunt(cin, nin)
    sh.part(ref, x0 + u(9), y + 2.54, fields=((0, -10.16, None), (0, -7.62, None)))
    pin_in, pin_en = sh.pin(R(ref), "1"), sh.pin(R(ref), "3")
    sh.wire(nin, pin_in); sh.wire(pin_in, pin_en); sh.junction(pin_in)
    sh.power("GND", sh.pin(R(ref), "2")); sh.nc(sh.pin(R(ref), "4"))
    o = sh.pin(R(ref), "5"); no = (o[0] + u(2), o[1]); sh.wire(o, no); shunt(cout, no)
    r = (no[0] + u(4), o[1]); sh.wire(no, r); sh.power(rail, r); sh.junction(r)
    if tp:
        t = (r[0] + u(3), r[1]); sh.wire(r, t); sh.part(tp, t[0], t[1], fields=((2.54, -3.81, "left"), (0, 0, None)))
ldo("U_LDOD", "C_LDD1", "C_LDD2", "V3D", ysp, u(170))
ldo("U_LDORF", "C_LDR1", "C_LDR2", "V3RF", u(124), u(170), tp="TP_3RF")
sh.text("ESP32-S3 rail", (u(170), ysp - u(4)), 1.27); sh.text("RF rail", (u(170), u(124) - u(4)), 1.27)
# USB-C receptacle: labels at the pins, CC pull-downs beside it
XU, YU = u(146), u(128)
sh.part("J_USB", XU, YU, fields=((0, -24.13, None), (0, -21.59, None)))
for n in ("A1", "S1"): sh.power("GND", sh.pin("J_USB1", n))
v = sh.pin("J_USB1", "A4"); e = stub(v, 5.08); sh.power("VBUS", e)
for n, net in (("A5", "CC1"), ("B5", "CC2")):
    e = stub(sh.pin("J_USB1", n), 5.08); sh.label(net, e)
for (pa, pb), net, dx in ((("A7", "B7"), "USB_DM", 2.54), (("A6", "B6"), "USB_DP", 5.08)):
    a, b = sh.pin("J_USB1", pa), sh.pin("J_USB1", pb); ta, tb = stub(a, dx), stub(b, dx); sh.wire(ta, tb); sh.junction(ta)
    e = (ta[0] + u(4) - dx, ta[1]); sh.wire(ta, e); sh.glabel(net, e)
for n in ("A8", "B8"): sh.nc(sh.pin("J_USB1", n))
for i, (net, r) in enumerate((("CC1", "R_CC1"), ("CC2", "R_CC2"))):
    s0 = (u(158) + u(7) * i, u(134)); sh.label(net, s0, 180); nn = (s0[0] + u(2), s0[1]); sh.wire(s0, nn); shunt(r, nn, junction=False)
sh.text("Rd 5.1k: USB-C sink", (u(156), u(131)), 1.0)
sh.part("U_ESD", u(204), u(126), fields=((-7.62, -7.62, "right"), (5.08, 7.62, "left")))
sh.power("VBUS", sh.pin("U_ESD1", "5")); sh.power("GND", sh.pin("U_ESD1", "2"))
for n, net, right in (("1", "USB_DP", False), ("6", "USB_DP", True), ("3", "USB_DM", False), ("4", "USB_DM", True)):
    e = stub(sh.pin("U_ESD1", n), 5.08 if right else -5.08); sh.glabel(net, e, right=right)
sh.text("USB ESD at the receptacle", (u(196), u(116)), 1.27)
# GND flag and test pad
tg = (u(214), u(104)); sh.part("TP_GND", tg[0], tg[1], fields=((2.54, -3.81, "left"), (0, 0, None)))
gd = (tg[0], tg[1] + 2.54); sh.wire(tg, gd); sh.power("GND", gd)
fl = (u(220), u(104)); sh.flag(fl); sh.wire(fl, (fl[0], fl[1] + 2.54)); sh.power("GND", (fl[0], fl[1] + 2.54))

# ============================================================================ notes
sh.block("Design notes", "", u(6), u(138), u(130), u(160))
sh.text("Spec: claude/SatHunter-FG23-OTG-nyak-spec-2026-11-07 (project) and hardware/fg23-otg/README.md.  Parts and nets: generator/design.py (single source of truth).\n"
        "Checks: check_netlist.py (drawing == design.py), twin_check.py (netlist vs datasheets / firmware / pin map, self-test with injected faults).\n"
        "To verify before layout: HDF434A1-S6 and QPL9547 land patterns vs the maker drawings; 560 nH choke SRF; TCXO and RF ESD part numbers in the JLC tool.\n"
        "Layout: 4 layers JLC04161H-7628, RF CPWG ~50 ohm, shield can over SAW1 ... FG23 + TCXO, ESP antenna at a board edge with copper keep-out.\n"
        "In-band spurs to measure on rev A: 440 MHz (ESP 40 MHz x11), 429 MHz (39 MHz x11, SPI 19.5 MHz x22), 432 / 444 MHz (USB 12 / 48 MHz).",
        (u(8), u(144)), 1.27)

# ---- split every wire at points that touch it from inside (wire ends, junctions, labels): the KiCad 7 netlister only
#      joins wires end to end, a T onto the middle of a segment stays open even with a junction dot
def _f(v): return float(v)
pts = set()
for it in sh.items:
    if it[0] == "wire":
        for xy in it[1][1:]: pts.add((_f(xy[1]), _f(xy[2])))
    elif it[0] in ("junction", "label", "global_label"):
        at = kfp.find(it, "at"); pts.add((_f(at[1]), _f(at[2])))
new = []
for it in sh.items:
    if it[0] != "wire": new.append(it); continue
    (x1, y1), (x2, y2) = [(_f(xy[1]), _f(xy[2])) for xy in it[1][1:]]
    inner = [q for q in pts if q not in ((x1, y1), (x2, y2)) and min(x1, x2) - 1e-6 <= q[0] <= max(x1, x2) + 1e-6 and min(y1, y2) - 1e-6 <= q[1] <= max(y1, y2) + 1e-6
             and abs((x2 - x1) * (q[1] - y1) - (y2 - y1) * (q[0] - x1)) < 1e-6]
    seq = sorted([(x1, y1)] + inner + [(x2, y2)], key=lambda q: (q[0] - x1) ** 2 + (q[1] - y1) ** 2)
    for a_, b_ in zip(seq, seq[1:]):
        new.append(["wire", ["pts", ["xy", g(a_[0]), g(a_[1])], ["xy", g(b_[0]), g(b_[1])]], ["stroke", ["width", "0"], ["type", "default"]], ["uuid", uid("w")]])
sh.items = new
missing = set(PARTS) - sh.placed
assert not missing, missing

# ============================================================================ write
used = {it[1][1].split(":")[1].strip('"') for it in sh.items if it[0] == "symbol"}
libsyms = [rename(SYMLIB[n], f"{LIB}:{n}") for n in sorted(used)]
for s in libsyms:   # sub-symbol names must not carry the library prefix
    for sub in kfp.findall(s, "symbol"): sub[1] = '"' + kfp.uq(sub[1]).replace(f"{LIB}:", "", 1) + '"'
def dump(x): return kfp.dump(x)
tb = ['title_block', ['title', '"SatHunter FG23 OTG - 70 cm satellite receiver"'], ['date', '"2026-11-07"'], ['rev', '"A"'], ['company', '"HA7DCD / Z2 Labs"'],
      ['comment', '1', '"SMA - SAW - QPL9547 - SAW - EFR32FG23 (RX) - ESP32-S3-WROOM-1 - USB-C OTG to the SatHunterSDR app"'],
      ['comment', '2', '"generated by generator/gen_sch.py from generator/design.py - do not hand-edit"'], ['comment', '3', '"CERN-OHL-P-2.0"']]
out = [f'(kicad_sch (version 20230121) (generator eeschema)\n(uuid "{ROOT}")\n(paper "A2")', dump(tb),
       "(lib_symbols\n" + "\n".join(dump(s) for s in libsyms) + "\n)"] + [dump(i) for i in sh.items] + ['(sheet_instances (path "/" (page "1")))\n)']
os.makedirs(OUT, exist_ok=True)
open(os.path.join(OUT, PRJ + ".kicad_sch"), "w").write("\n".join(out))
open(os.path.join(OUT, "SH.kicad_sym"), "w").write("(kicad_symbol_lib (version 20220914) (generator kicad_symbol_editor)\n" +
     "\n".join(dump(rename(SYMLIB[n], n)) for n in sorted(used)) + "\n)\n")
os.makedirs(os.path.join(OUT, "SH.pretty"), exist_ok=True)
for fp, spec in design.FPMAP.items():
    if spec.endswith(".kicad_mod"): shutil.copy(os.path.join(HERE, spec), os.path.join(OUT, "SH.pretty", os.path.basename(spec)))
open(os.path.join(OUT, "fp-lib-table"), "w").write('(fp_lib_table\n  (version 7)\n  (lib (name "SH")(type "KiCad")(uri "${KIPRJMOD}/SH.pretty")(options "")(descr "SatHunter FG23 project footprints"))\n)\n')
open(os.path.join(OUT, "sym-lib-table"), "w").write('(sym_lib_table\n  (version 7)\n  (lib (name "SH")(type "KiCad")(uri "${KIPRJMOD}/SH.kicad_sym")(options "")(descr "SatHunter FG23 symbols"))\n)\n')
pro_path = os.path.join(OUT, PRJ + ".kicad_pro")
pro = json.load(open(pro_path)) if os.path.exists(pro_path) else {}
pro.setdefault("meta", {"filename": PRJ + ".kicad_pro", "version": 1}); pro["sheets"] = [[ROOT, ""]]
pro.setdefault("schematic", {"legacy_lib_dir": "", "legacy_lib_list": []})
json.dump(pro, open(pro_path, "w"), indent=2)
open(os.path.join(OUT, "dnp.txt"), "w").write("\n".join(r for r, p in PARTS.items() if p["dnp"]) + "\n")
bom = ["Ref,Value,Footprint,DNP,Notes"]
for ref, p in PARTS.items(): bom.append(f'{ref},"{p["value"]}",{fp_lib(p["fp"])},{"DNP" if p["dnp"] else ""},"{p["desc"]}"')
open(os.path.join(OUT, "BOM.csv"), "w").write("\n".join(bom) + "\n")
print("gen_sch:", len(sh.placed), "parts,", sh.npwr, "power symbols ->", os.path.abspath(OUT))
