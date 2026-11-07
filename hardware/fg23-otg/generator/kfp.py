"""Load official KiCad footprints (.kicad_mod) -> pads + raw graphics/model s-exprs, for the generator."""
import re, os
LIB=os.environ.get("KICAD_FP_DIR","/usr/share/kicad/footprints")
HERE=os.path.dirname(os.path.abspath(__file__))

def _tok(s):
    return re.findall(r'\(|\)|"(?:[^"\\]|\\.)*"|[^\s()]+', s)
def parse(s):
    t=_tok(s); i=0
    def rd():
        nonlocal i
        assert t[i]=="("; i+=1; out=[]
        while t[i]!=")":
            if t[i]=="(": out.append(rd())
            else: out.append(t[i]); i+=1
        i+=1; return out
    return rd()
def dump(x):
    if isinstance(x,list): return "("+" ".join(dump(e) for e in x)+")"
    return x
def find(n,key):
    for e in n:
        if isinstance(e,list) and e and e[0]==key: return e
def findall(n,key): return [e for e in n if isinstance(e,list) and e and e[0]==key]
def uq(s): return s[1:-1] if s.startswith('"') else s

_cache={}
def load(spec):
    """spec 'Lib:Name' (official) or 'path/to/file.kicad_mod'. Returns dict(pads=[...], items=[sexpr...], name)."""
    if spec in _cache: return _cache[spec]
    if ":" in spec and not spec.endswith(".kicad_mod"):
        lib,name=spec.split(":",1); path=os.path.join(LIB,lib+".pretty",name+".kicad_mod")
    else:
        path=spec if os.path.isabs(spec) else os.path.join(HERE,spec); name=os.path.basename(spec)[:-10]
    tree=parse(open(path,encoding="utf-8").read())
    pads=[]; items=[]
    for e in tree[2:]:
        if not isinstance(e,list): continue
        k=e[0]
        if k=="pad":
            num=uq(e[1]); typ=e[2]; shape=e[3]
            at=find(e,"at"); x,y=float(at[1]),float(at[2]); rot=float(at[3]) if len(at)>3 else 0.0
            sz=find(e,"size"); w,h=float(sz[1]),float(sz[2])
            dr=find(e,"drill"); drill=0.0; oval=None
            if dr:
                if dr[1]=="oval": drill=float(dr[2]); oval=(float(dr[2]),float(dr[3]))
                else: drill=float(dr[1])
            layers=[uq(l) for l in find(e,"layers")[1:]]
            rr=find(e,"roundrect_rratio")
            pads.append(dict(num=num,type=typ,shape=shape,x=x,y=y,rot=rot,w=w,h=h,drill=drill,oval=oval,layers=layers,
                             rratio=float(rr[1]) if rr else None, raw=e))
        elif k in ("fp_line","fp_rect","fp_circle","fp_arc","fp_poly","fp_text","model"):
            items.append(e)
        elif k=="attr": attr=e
    d=dict(pads=pads,items=items,name=name,path=path,attr=locals().get("attr",["attr","smd"]))
    _cache[spec]=d; return d

def bbox(spec, layers=("F.CrtYd",)):
    d=load(spec); xs=[];ys=[]
    for e in d["items"]:
        ly=find(e,"layer")
        if not ly or uq(ly[1]) not in layers: continue
        for key in ("start","end","center","mid"):
            p=find(e,key)
            if p: xs.append(float(p[1])); ys.append(float(p[2]))
        pts=find(e,"pts")
        if pts:
            for xy in findall(pts,"xy"): xs.append(float(xy[1])); ys.append(float(xy[2]))
    return (min(xs),min(ys),max(xs),max(ys)) if xs else None

if __name__=="__main__":
    import sys
    for s in sys.argv[1:]:
        d=load(s); print("==",s, "crtyd",bbox(s))
        for p in d["pads"]: print("  ",p["num"],p["type"],p["shape"],p["x"],p["y"],p["rot"],p["w"],p["h"],p["drill"],p["oval"],p["layers"][:2])
        for e in d["items"]:
            if e[0]=="model": print("   model",dump(e)[:200])
