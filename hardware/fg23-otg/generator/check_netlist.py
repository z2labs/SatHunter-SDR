#!/usr/bin/env python3
"""Generator self-check: the netlist KiCad extracts from the drawn schematic must equal the generator's PARTS table.
Catches drawing faults (touching label stubs, wires through pins) that silently merge or split nets."""
import sys, os, importlib.util
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE); import kfp
def load_parts():
    import design
    return design.PARTS
def netlist(path):
    t = kfp.parse(open(path).read()); out = {}
    for n in kfp.findall(kfp.find(t, "nets"), "net"):
        name = kfp.uq(kfp.find(n, "name")[1]).lstrip("/")      # local labels on the root sheet: "/NAME"
        for x in kfp.findall(n, "node"): out[(kfp.uq(kfp.find(x, "ref")[1]), kfp.uq(kfp.find(x, "pin")[1]))] = name
    return out
def main(net_path):
    parts = load_parts(); nl = netlist(net_path); bad = []
    groups = {}
    for ref, p in parts.items():
        for pin, net in p["pins"].items():
            got = nl.get((ref, pin))
            if got is None: bad.append(f"{ref}.{pin}: missing in netlist"); continue
            if net == "":
                if not got.startswith("unconnected-"): bad.append(f"{ref}.{pin}: should be NC, is on {got}")
                continue
            groups.setdefault(net, set()).add(got)
    for net, got in groups.items():
        if len(got) != 1: bad.append(f"net {net} split into {sorted(got)}")
        elif next(iter(got)) != net: bad.append(f"net {net} appears as {next(iter(got))} (merged with another net?)")
    rev = {}
    for net, got in groups.items():
        for gname in got: rev.setdefault(gname, set()).add(net)
    for gname, nets in rev.items():
        if len(nets) > 1: bad.append(f"SHORT: {sorted(nets)} are one net ({gname})")
    extra = {k for k in nl if k[0] not in parts and not k[0].startswith("#")}
    if extra: bad.append(f"netlist has unknown refs {sorted(extra)[:5]}")
    print(f"check_netlist: {sum(len(p['pins']) for p in parts.values())} pins, {len(groups)} nets, {len(bad)} problems")
    for b in bad: print("  FAIL", b)
    return 1 if bad else 0
if __name__ == "__main__": sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "..", "kicad", "netlist.net")))
