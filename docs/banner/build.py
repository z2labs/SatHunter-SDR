import math, random, base64, html
random.seed(3)
wf=base64.b64encode(open('wf.png','rb').read()).decode()

# --- TLE "database" text (real TLE layout, made-up values) ---
names=["ISS (ZARYA)","MAUVE","FUNCUBE-1 (AO-73)","SO-50","AO-91","RS-44","CAS-4A","XW-2B","LILACSAT-2","TEVEL-1","GREENCUBE","UVSQ-SAT","NAYIF-1","JY1SAT","PO-101","IO-117","SONATE-2","CUTE","FOSSASAT-2E","SMOG-1"]
def tle(n,norad):
    yr=random.choice([98,13,17,18,19,20,22,23,24]); epoch=f"26{random.uniform(290,300):012.8f}"
    l1=f"1 {norad:05d}U {yr:02d}{random.randint(1,99):03d}A   {epoch}  .{random.randint(1000,99999):08d}  00000-0  {random.randint(10000,99999)}-3 0  999{random.randint(0,9)}"
    l2=f"2 {norad:05d}  {random.uniform(40,99):7.4f} {random.uniform(0,360):8.4f} {random.randint(1000,9999999):07d} {random.uniform(0,360):8.4f} {random.uniform(0,360):8.4f} {random.uniform(14,16):11.8f}{random.randint(10000,99999)}"
    return [n,l1,l2]
lines=[]
for k in range(16):
    for i,n in enumerate(names):
        lines+=tle(n,random.choice([25544,66756,39444,27607,43017,44909,42761,40903,40908,51013,53106,47438,42017,43803,47963,53057,59112,49263,56973,59054]))
tl=[]
for i,l in enumerate(lines[:60]):
    bright = 'hl' if i in (9,10,11,33,34,35) else ''
    tl.append(f'<div class="{bright}">{html.escape(l)}</div>')
tle_html="".join(tl)

# --- the boy, the Yagi, the phone ---
ox=40  # shift the whole figure
def P(s, off=0.0):
    # point along the boom (s px) and across it (off px)
    ux,uy=0.819,-0.574; nx,ny=0.574,0.819
    bx,by=700+ox,360
    return (bx+s*ux+off*nx, by+s*uy+off*ny)
def ln(a,b,**kw):
    attrs=" ".join(f'{k.replace("_","-")}="{v}"' for k,v in kw.items())
    return f'<line x1="{a[0]:.1f}" y1="{a[1]:.1f}" x2="{b[0]:.1f}" y2="{b[1]:.1f}" {attrs}/>'
el=[]
# elements: reflector, driven, 3 directors (lengths in px, perpendicular to the boom)
for s,L,cls in [(70,158,'el'),(150,146,'drv'),(232,134,'el'),(316,126,'el'),(400,120,'el')]:
    if cls=='drv':
        el.append(ln(P(s,-L/2),P(s,-4),stroke="#d7dde4",stroke_width=5,stroke_linecap="round"))
        el.append(ln(P(s,4),P(s,L/2),stroke="#d7dde4",stroke_width=5,stroke_linecap="round"))
        el.append(f'<rect x="{P(s)[0]-7:.1f}" y="{P(s)[1]-7:.1f}" width="14" height="14" rx="3" fill="#2a3644" transform="rotate(-35 {P(s)[0]:.1f} {P(s)[1]:.1f})"/>')
    else:
        el.append(ln(P(s,-L/2),P(s,L/2),stroke="#d7dde4",stroke_width=5,stroke_linecap="round"))
    el.append(f'<circle cx="{P(s)[0]:.1f}" cy="{P(s)[1]:.1f}" r="5" fill="#9aa6b2"/>')
boom=ln(P(-60),P(420),stroke="#e9eef3",stroke_width=11,stroke_linecap="round")
boom_shade=ln(P(-60,3),P(420,3),stroke="#8794a3",stroke_width=4,stroke_linecap="round")
handle=ln(P(-60),P(30),stroke="#22303e",stroke_width=15,stroke_linecap="round")
# coax from the driven element back to the phone
c0=P(150,0); c1=P(118,10); c2=P(100,16)
coax=f'<path d="M{c0[0]:.1f},{c0[1]:.1f} Q{c1[0]:.1f},{c1[1]+14:.1f} {c2[0]:.1f},{c2[1]:.1f}" stroke="#111" stroke-width="4" fill="none"/>'
# phone clamped on the boom between hands and reflector, screen with the radar
pc=P(112,-30)
phone=f'''
<g transform="translate({pc[0]:.1f},{pc[1]:.1f}) rotate(-35)">
  <rect x="-38" y="-24" width="76" height="48" rx="7" fill="#0c1117" stroke="#3b4a5a" stroke-width="2.5"/>
  <rect x="-33" y="-19" width="66" height="38" rx="4" fill="#06140d"/>
  <g transform="translate(-14,0)">
    <circle r="15" fill="none" stroke="#2fd47a" stroke-width="1.3" opacity=".9"/>
    <circle r="9" fill="none" stroke="#2fd47a" stroke-width="1" opacity=".7"/>
    <line x1="-15" y1="0" x2="15" y2="0" stroke="#2fd47a" stroke-width=".8" opacity=".6"/>
    <line x1="0" y1="-15" x2="0" y2="15" stroke="#2fd47a" stroke-width=".8" opacity=".6"/>
    <path d="M-10,9 Q0,-14 12,-4" stroke="#ffc83c" stroke-width="1.4" fill="none"/>
    <circle cx="6" cy="-6" r="2.6" fill="#ffc83c"/>
  </g>
  <rect x="5" y="-12" width="22" height="4" rx="1" fill="#ff9a3c"/>
  <rect x="5" y="-4" width="16" height="3" rx="1" fill="#4fb3ff"/>
  <rect x="5" y="3" width="20" height="3" rx="1" fill="#4fb3ff" opacity=".7"/>
  <rect x="5" y="10" width="12" height="3" rx="1" fill="#4fb3ff" opacity=".5"/>
  <rect x="-10" y="22" width="20" height="9" rx="2" fill="#555"/>
</g>'''
# RTL-SDR dongle hanging from the phone on a short cable
dg=P(70,40)
dongle=f'''<path d="M{P(100,-6)[0]:.1f},{P(100,-6)[1]:.1f} C{P(95,25)[0]:.1f},{P(95,25)[1]:.1f} {P(80,30)[0]:.1f},{P(80,30)[1]:.1f} {dg[0]:.1f},{dg[1]:.1f}" stroke="#111" stroke-width="3" fill="none"/>
<g transform="translate({dg[0]:.1f},{dg[1]:.1f}) rotate(-80)"><rect x="-6" y="-24" width="13" height="40" rx="3" fill="#1f6fd1"/><rect x="-3" y="-30" width="7" height="8" fill="#c7cdd4"/></g>'''

X=lambda x: x+ox
boy=f'''
<!-- back arm (behind the body) -->
<path d="M{X(565)},432 Q{X(640)},420 {P(30)[0]:.1f},{P(30)[1]+8:.1f}" stroke="#c9671f" stroke-width="40" stroke-linecap="round" fill="none"/>
<circle cx="{P(30)[0]:.1f}" cy="{P(30)[1]:.1f}" r="17" fill="#e9b48e"/>
<!-- body: hoodie -->
<path d="M{X(470)},640 C{X(470)},520 {X(495)},430 {X(560)},410 L{X(650)},408 C{X(705)},425 {X(720)},520 {X(722)},640 Z" fill="#f08a2c"/>
<path d="M{X(560)},410 C{X(575)},450 {X(625)},455 {X(650)},408" fill="#d9731c"/>

<!-- hood behind the neck -->
<path d="M{X(530)},425 C{X(522)},392 {X(548)},374 {X(585)},378 L{X(620)},385 C{X(640)},392 {X(650)},404 {X(652)},412 C{X(610)},430 {X(565)},432 {X(530)},425 Z" fill="#c9671f"/>
<!-- pocket and drawstrings -->
<path d="M{X(540)},560 Q{X(600)},545 {X(670)},560 L{X(682)},640 L{X(528)},640 Z" fill="#dc7720"/>
<line x1="{X(588)}" y1="420" x2="{X(584)}" y2="470" stroke="#fff3e3" stroke-width="3" stroke-linecap="round"/>
<line x1="{X(616)}" y1="420" x2="{X(620)}" y2="466" stroke="#fff3e3" stroke-width="3" stroke-linecap="round"/>
<!-- neck -->
<rect x="{X(578)}" y="372" width="34" height="40" rx="12" fill="#e9b48e"/>
<!-- head, looking up toward the satellite -->
<g transform="rotate(-14 {X(600)} 320)">
  <ellipse cx="{X(600)}" cy="318" rx="62" ry="64" fill="#f3c9a3"/>
  <!-- hair -->
  <path d="M{X(540)},322 C{X(530)},262 {X(585)},240 {X(625)},248 C{X(655)},252 {X(668)},270 {X(664)},292 C{X(645)},280 {X(620)},282 {X(610)},268 C{X(598)},285 {X(575)},290 {X(560)},300 Z" fill="#5a3a24"/>
  <path d="M{X(540)},322 C{X(536)},300 {X(540)},285 {X(548)},275 L{X(560)},300 Z" fill="#4a2f1c"/>
  <!-- eye, brow, nose, smile, cheek -->
  <ellipse cx="{X(636)}" cy="313" rx="5.5" ry="7" fill="#24180f"/>
  <circle cx="{X(638)}" cy="310" r="2" fill="#fff"/>
  <path d="M{X(626)},298 Q{X(637)},292 {X(648)},297" stroke="#4a2f1c" stroke-width="3.5" fill="none" stroke-linecap="round"/>
  <path d="M{X(660)},318 Q{X(668)},330 {X(659)},334" stroke="#d99a76" stroke-width="3" fill="none" stroke-linecap="round"/>
  <path d="M{X(632)},346 Q{X(645)},356 {X(656)},344" stroke="#9a4a3a" stroke-width="3.5" fill="none" stroke-linecap="round"/>
  <ellipse cx="{X(622)}" cy="334" rx="11" ry="7" fill="#f29b8c" opacity=".55"/>
  <!-- headphones: band over the head, cup over the ear -->
  <path d="M{X(560)},330 C{X(548)},250 {X(640)},222 {X(664)},270" stroke="#1d2833" stroke-width="12" fill="none" stroke-linecap="round"/>
  <path d="M{X(560)},330 C{X(548)},250 {X(640)},222 {X(664)},270" stroke="#4fb3ff" stroke-width="3" fill="none" stroke-linecap="round" opacity=".8"/>
  <ellipse cx="{X(568)}" cy="330" rx="24" ry="30" fill="#1d2833"/>
  <ellipse cx="{X(568)}" cy="330" rx="15" ry="20" fill="#4fb3ff"/>
  <ellipse cx="{X(568)}" cy="330" rx="7" ry="10" fill="#2a8fd8"/>
</g>
<!-- front arm -->
<path d="M{X(640)},432 Q{X(700)},440 {P(-28)[0]:.1f},{P(-28)[1]+12:.1f}" stroke="#f08a2c" stroke-width="42" stroke-linecap="round" fill="none"/>
<circle cx="{P(-28)[0]:.1f}" cy="{P(-28)[1]:.1f}" r="18" fill="#f3c9a3"/>
'''
sat=f'''
<g transform="translate(1188,52) rotate(-25)">
  <rect x="-48" y="-10" width="34" height="20" fill="#2a64c9" stroke="#9cc4ff" stroke-width="1.5"/>
  <line x1="-41" y1="-10" x2="-41" y2="10" stroke="#9cc4ff" stroke-width=".8"/><line x1="-31" y1="-10" x2="-31" y2="10" stroke="#9cc4ff" stroke-width=".8"/><line x1="-22" y1="-10" x2="-22" y2="10" stroke="#9cc4ff" stroke-width=".8"/>
  <rect x="14" y="-10" width="34" height="20" fill="#2a64c9" stroke="#9cc4ff" stroke-width="1.5"/>
  <line x1="22" y1="-10" x2="22" y2="10" stroke="#9cc4ff" stroke-width=".8"/><line x1="31" y1="-10" x2="31" y2="10" stroke="#9cc4ff" stroke-width=".8"/><line x1="41" y1="-10" x2="41" y2="10" stroke="#9cc4ff" stroke-width=".8"/>
  <rect x="-14" y="-14" width="28" height="28" rx="3" fill="#d8dee6" stroke="#fff" stroke-width="1"/>
  <rect x="-8" y="-8" width="16" height="16" rx="2" fill="#ffb347"/>
  <line x1="0" y1="14" x2="0" y2="32" stroke="#fff" stroke-width="2"/>
</g>'''
import math as _m
def arc(cx,cy,r,a0,a1):
    p0=(cx+r*_m.cos(_m.radians(a0)),cy+r*_m.sin(_m.radians(a0))); p1=(cx+r*_m.cos(_m.radians(a1)),cy+r*_m.sin(_m.radians(a1)))
    return f'M{p0[0]:.1f},{p0[1]:.1f} A{r},{r} 0 0 1 {p1[0]:.1f},{p1[1]:.1f}'
waves="".join(f'<path d="{arc(1188,52,30+22*k,122,172)}" stroke="#ffd25a" stroke-width="4" stroke-linecap="round" fill="none" opacity="{1.0-0.22*k:.2f}"/>' for k in range(1,4))
_old="".join(f'<path d="M{1188-28*k*0.82-8:.0f},{60+28*k*0.57+6:.0f} a {22*k} {22*k} 0 0 0 {22*k*0.9:.0f} {-22*k*1.0:.0f}" stroke="#ffc83c" stroke-width="3" fill="none" opacity="{1-0.22*k:.2f}" transform="rotate(10 1188 52)"/>' for k in range(1,4))
stars="".join(f'<circle cx="{random.uniform(480,1280):.0f}" cy="{random.uniform(0,230):.0f}" r="{random.choice([0.8,1,1.2,1.6]):.1f}" fill="#fff" opacity="{random.uniform(.3,.9):.2f}"/>' for _ in range(60))

page=f'''<!doctype html><html><head><meta charset="utf-8"><style>
html,body{{margin:0;width:1280px;height:640px;overflow:hidden;background:#05080c}}
.wf{{position:absolute;inset:0;background:url(data:image/png;base64,{wf}) center/cover;filter:saturate(1.15) brightness(.95);
  -webkit-mask-image:linear-gradient(90deg,transparent 0%,rgba(0,0,0,.25) 22%,rgba(0,0,0,.9) 52%,#000 70%)}}
.sky{{position:absolute;inset:0;background:linear-gradient(180deg,rgba(5,8,12,.55) 0%,rgba(5,8,12,0) 40%),radial-gradient(ellipse at 52% 78%,rgba(5,8,12,.55),rgba(5,8,12,0) 55%)}}
.tle{{position:absolute;left:10px;top:-6px;width:470px;font:10.5px/10.9px "DejaVu Sans Mono",monospace;color:#4fb3ff;white-space:pre;opacity:.42;
  -webkit-mask-image:linear-gradient(90deg,#000 55%,transparent 100%)}}
.tle div:nth-child(3n){{opacity:.6}} .tle .hl{{color:#bfe3ff;text-shadow:0 0 6px #4fb3ff;opacity:1}}
.shade{{position:absolute;left:0;top:0;width:620px;height:640px;background:linear-gradient(90deg,rgba(5,8,12,.2),rgba(5,8,12,.55) 55%,rgba(5,8,12,0))}}
svg{{position:absolute;inset:0}}
.title{{position:absolute;left:48px;top:178px;font-family:Inter,sans-serif;color:#eef3f8}}
h1{{margin:0;font-size:66px;font-weight:800;letter-spacing:-1.5px;line-height:1;text-shadow:0 2px 18px rgba(0,0,0,.8)}}
h1 b{{color:#ff9a3c;text-shadow:0 0 18px rgba(255,90,0,.6)}}
.tag{{margin-top:14px;font-size:25px;font-weight:600;text-shadow:0 2px 10px #000}}
.sub{{margin-top:12px;font-size:16.5px;color:#a9b6c3;line-height:1.5;text-shadow:0 1px 6px #000}}
.url{{position:absolute;left:48px;bottom:30px;font:15px "DejaVu Sans Mono",monospace;color:#8fa0b0}}
</style></head><body>
<div class="wf"></div><div class="sky"></div>
<div class="tle">{tle_html}</div>
<div class="shade"></div>
<svg width="1280" height="640" viewBox="0 0 1280 640">
<defs><filter id="glow"><feGaussianBlur stdDeviation="3" result="b"/><feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge></filter></defs>
{stars}
<path d="M560,250 Q900,-40 1280,20" stroke="#9cc4ff" stroke-width="1.6" stroke-dasharray="6 7" fill="none" opacity=".55"/>
<g filter="url(#glow)">{sat}</g>
{waves}
<g>{boy}</g>
{handle}{boom}{boom_shade}{"".join(el)}{coax}{phone}{dongle}
</svg>
<div class="title"><h1>SatHunter<b>SDR</b></h1>
<div class="tag">Hunt satellites with your phone.</div>
<div class="sub">RTL-SDR or ESP32-S3 over USB OTG · live TLEs<br>Doppler tracking · Look4Sat-style radar</div></div>
<div class="url">github.com/z2labs/SatHunter-SDR</div>
</body></html>'''
open('banner.html','w').write(page)
from playwright.sync_api import sync_playwright
with sync_playwright() as p:
    b=p.chromium.launch(); pg=b.new_page(viewport={'width':1280,'height':640})
    pg.goto('file://'+__import__('os').path.abspath('banner.html')); pg.wait_for_timeout(300)
    pg.screenshot(path='banner.png'); b.close()
print('done')
