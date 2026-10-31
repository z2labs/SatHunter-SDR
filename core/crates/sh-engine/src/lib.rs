//! SatHunterSDR engine: owns the source, the DSP thread, the satellite tracker and the settings.
//! The UI (Android today, desktop later) talks to it with JSON commands and pulls spectrum rows
//! and audio. Nothing in here panics on bad input; errors come back as {"ok": false, "error"}.

pub mod config;
pub mod sat;

use config::Config;
use sat::{Other, SatState};
use serde::Deserialize;
use serde_json::{json, Value};
use sh_dsp::demod::{Demodulator, Mode};
use sh_dsp::spectrum::Spectrum;
use sh_sat::orbit::Station;
use sh_sat::tracker::{demod_for, waiting_for_aos};
use sh_sat::{locator, Filter};
use sh_sources::demo::{sat_slot, DemoSource};
use sh_sources::{SatSignal, SharedSatSignal, SignalKind, Sink, Source};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const AUDIO_MAX: usize = 48_000; // 1 s
const ROWS_MAX: usize = 64;

pub fn now_unix() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Lock that survives a panic in another thread (the data is still usable for a receiver).
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the DSP thread should do; `version` changes on every edit.
#[derive(Debug, Clone)]
struct DspParams {
    version: u64,
    mode: Mode,
    bw: f64,
    vfo_hz: f64,
    fft_size: usize,
    fps: f64,
    squelch: Option<f32>,
}

#[derive(Debug, Default)]
struct DspOut {
    level_db: f32,
    bins: usize,
    center_hz: f64,
    rate: f64,
}

enum Work {
    Download,
    Recalc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
struct UsbDev {
    kind: &'static str,
    vid: u16,
    pid: u16,
    fd: Option<i32>,
    blog_v4: bool,
}

struct Inner {
    devices: Mutex<Vec<UsbDev>>,
    dir: PathBuf,
    cfg: Mutex<Config>,
    source: Mutex<Option<Box<dyn Source>>>,
    params: Mutex<DspParams>,
    out: Mutex<DspOut>,
    /// FFT rows with the centre frequency and sample rate they were taken at
    rows: Mutex<VecDeque<(f64, f64, Vec<f32>)>>,
    audio: Mutex<VecDeque<f32>>,
    sat: Mutex<SatState>,
    sat_signal: SharedSatSignal,
    dsp_thread: Mutex<Option<JoinHandle<()>>>,
    dropped: Mutex<Option<Arc<AtomicU64>>>,
    running: AtomicBool,
    quit: AtomicBool,
    time_shift: Mutex<(f64, f64)>, // (seconds added to the clock, LOS of the simulated pass)
    work: Mutex<Sender<Work>>,
    messages: Mutex<VecDeque<String>>,
}

impl Inner {
    fn sim_now(&self) -> f64 {
        now_unix() + lock(&self.time_shift).0
    }

    fn note(&self, s: impl Into<String>) {
        let s = s.into();
        log::info!("{s}");
        let mut m = lock(&self.messages);
        m.push_back(s);
        while m.len() > 200 {
            m.pop_front();
        }
    }

    fn save(&self) {
        let c = lock(&self.cfg).clone();
        c.save(&self.dir.join("config.json"));
    }

    fn bump_params(&self, f: impl FnOnce(&mut DspParams)) {
        let mut p = lock(&self.params);
        f(&mut p);
        p.version += 1;
    }
}

pub struct Engine {
    inner: Arc<Inner>,
    threads: Vec<JoinHandle<()>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
enum Cmd {
    Start,
    Stop,
    /// Tune the VFO (absolute Hz); the hardware follows when needed
    Tune { hz: f64 },
    /// Hardware centre frequency
    Center { hz: f64 },
    Mode { mode: String, bw: Option<f64> },
    Bandwidth { hz: f64 },
    Gain { db: Option<f32> },
    /// 4.5 V on the antenna input for an LNA (RTL-SDR)
    BiasTee { on: bool },
    /// Receiver frequency correction, ppm
    Ppm { ppm: f64 },
    Rate { hz: f64 },
    Fft { size: usize },
    Squelch { db: Option<f32> },
    Station { lat: f64, lon: f64, alt_m: Option<f64> },
    Locator { loc: String },
    Gps { lat: f64, lon: f64, alt_m: f64 },
    FollowGps { on: bool },
    Filter { filter: Filter },
    Favourite { norad: u64, on: bool },
    SatUpdate,
    SatSelect { norad: u64, tx: Option<String> },
    SatTrack { on: bool },
    SatResetOffset,
    Simulate,
    RealTime,
    StepHz { hz: f64 },
    AutoDemod { on: bool },
    /// Keep the VFO in the middle: tuning moves the hardware (like SDR++ "Center Tuning")
    CenterTuning { on: bool },
    SatList,
    SatDetail,
    Status,
    Messages,
    Config,
    /// Android: a USB SDR was plugged in and permitted (fd from UsbDeviceConnection)
    UsbAttach { fd: Option<i32>, vid: u16, pid: u16, product: Option<String> },
    UsbDetach { vid: u16, pid: u16 },
    /// Choose the source: demo, rtl, esp
    Source { id: String },
}

impl Engine {
    pub fn new(dir: &Path) -> Engine {
        let _ = std::fs::create_dir_all(dir);
        let cfg = Config::load(&dir.join("config.json"));
        let mode = Mode::parse(&cfg.mode).unwrap_or(Mode::Wfm);
        let (wtx, wrx) = channel();
        let inner = Arc::new(Inner {
            dir: dir.to_path_buf(),
            params: Mutex::new(DspParams {
                version: 1,
                mode,
                bw: cfg.bw,
                vfo_hz: cfg.vfo_hz,
                fft_size: cfg.fft_size,
                fps: cfg.fps,
                squelch: cfg.squelch,
            }),
            out: Mutex::new(DspOut::default()),
            rows: Mutex::new(VecDeque::new()),
            audio: Mutex::new(VecDeque::new()),
            sat: Mutex::new(SatState::default()),
            sat_signal: sat_slot(),
            source: Mutex::new(None),
            dsp_thread: Mutex::new(None),
            dropped: Mutex::new(None),
            running: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            time_shift: Mutex::new((0.0, 0.0)),
            work: Mutex::new(wtx),
            messages: Mutex::new(VecDeque::new()),
            devices: Mutex::new(Vec::new()),
            cfg: Mutex::new(cfg),
        });
        {
            let c = lock(&inner.cfg);
            let mut s = lock(&inner.sat);
            s.tracker.step_hz = c.step_hz;
        }
        let mut threads = Vec::new();
        let i1 = inner.clone();
        if let Ok(t) = std::thread::Builder::new().name("sat-worker".into()).spawn(move || sat_worker(i1, wrx)) {
            threads.push(t);
        }
        let i2 = inner.clone();
        if let Ok(t) = std::thread::Builder::new().name("control".into()).spawn(move || control(i2)) {
            threads.push(t);
        }
        Engine { inner, threads }
    }

    /// Run a JSON command; always returns JSON.
    pub fn command(&self, json: &str) -> String {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match serde_json::from_str::<Cmd>(json) {
            Ok(c) => self.run(c),
            Err(e) => Err(format!("bad command: {e}")),
        }));
        let v = match r {
            Ok(Ok(v)) => {
                let mut v = if v.is_object() { v } else { json!({ "value": v }) };
                v["ok"] = json!(true);
                v
            }
            Ok(Err(e)) => json!({ "ok": false, "error": e }),
            Err(_) => json!({ "ok": false, "error": "internal error" }),
        };
        v.to_string()
    }

    /// Copy new spectrum rows (oldest first) into `out`, and each row's centre frequency and
    /// sample rate into `meta` (2 values per row, when it has room). Returns (rows, bins per row).
    pub fn spectrum_rows(&self, out: &mut [f32], meta: &mut [f64]) -> (usize, usize) {
        let mut q = lock(&self.inner.rows);
        let Some(bins) = q.front().map(|r| r.2.len()) else { return (0, 0) };
        let mut n = 0;
        while let Some((c, rate, r)) = q.front() {
            if r.len() != bins || (n + 1) * bins > out.len() || (!meta.is_empty() && 2 * n + 2 > meta.len()) {
                break;
            }
            out[n * bins..(n + 1) * bins].copy_from_slice(r);
            if !meta.is_empty() {
                meta[2 * n] = *c;
                meta[2 * n + 1] = *rate;
            }
            q.pop_front();
            n += 1;
        }
        if n == 0 && bins > out.len() {
            q.pop_front(); // row larger than the caller's buffer: drop it
        }
        (n, bins)
    }

    /// Pull up to out.len() audio samples (48 kHz mono). Returns samples written.
    pub fn audio(&self, out: &mut [f32]) -> usize {
        let mut q = lock(&self.inner.audio);
        let n = out.len().min(q.len());
        for (o, s) in out.iter_mut().zip(q.drain(..n)) {
            *o = s;
        }
        n
    }

    fn run(&self, c: Cmd) -> Result<Value, String> {
        let i = &self.inner;
        match c {
            Cmd::Start => {
                start(i)?;
                Ok(json!({}))
            }
            Cmd::Stop => {
                stop(i);
                Ok(json!({}))
            }
            Cmd::Tune { hz } => {
                tune(i, hz)?;
                Ok(json!({ "vfo_hz": lock(&i.cfg).vfo_hz }))
            }
            Cmd::Center { hz } => {
                let (min, max) = lock(&i.source).as_ref().map(|s| (s.info().min_hz, s.info().max_hz)).unwrap_or((0.0, f64::MAX));
                let c = set_center(i, hz.clamp(min, max))?;
                if lock(&i.cfg).center_tuning {
                    // Centre tuning: the VFO rides in the middle of the span
                    lock(&i.cfg).vfo_hz = c;
                    i.bump_params(|p| p.vfo_hz = c);
                    return Ok(json!({ "center_hz": c, "vfo_hz": c }));
                }
                // The VFO stays where it is unless it would leave the received span: then it is
                // pushed along at the edge
                let (rate, bw, vfo) = {
                    let cfg = lock(&i.cfg);
                    (cfg.rate, cfg.bw, cfg.vfo_hz)
                };
                let edge = (rate * 0.45 - bw / 2.0).max(rate * 0.05);
                let v = vfo.clamp(c - edge, c + edge);
                if v != vfo {
                    lock(&i.cfg).vfo_hz = v;
                    i.bump_params(|p| p.vfo_hz = v);
                }
                Ok(json!({ "center_hz": c, "vfo_hz": v }))
            }
            Cmd::Mode { mode, bw } => {
                let m = Mode::parse(&mode).ok_or("unknown mode")?;
                let bw = bw.unwrap_or(m.default_bw());
                {
                    let mut c = lock(&i.cfg);
                    c.mode = m.name().into();
                    c.bw = bw;
                }
                i.bump_params(|p| {
                    p.mode = m;
                    p.bw = bw;
                });
                i.save();
                Ok(json!({}))
            }
            Cmd::Bandwidth { hz } => {
                let hz = hz.clamp(100.0, 250_000.0);
                lock(&i.cfg).bw = hz;
                i.bump_params(|p| p.bw = hz);
                i.save();
                Ok(json!({}))
            }
            Cmd::BiasTee { on } => {
                lock(&i.cfg).bias_tee = on;
                if let Some(s) = lock(&i.source).as_mut() {
                    if s.has_bias_tee() {
                        s.set_bias_tee(on).map_err(|e| e.to_string())?;
                    }
                }
                i.save();
                Ok(json!({}))
            }
            Cmd::Ppm { ppm } => {
                let ppm = ppm.clamp(-200.0, 200.0);
                lock(&i.cfg).ppm = ppm;
                if let Some(s) = lock(&i.source).as_mut() {
                    s.set_ppm(ppm).map_err(|e| e.to_string())?;
                }
                i.save();
                Ok(json!({}))
            }
            Cmd::Gain { db } => {
                lock(&i.cfg).gain = db;
                if let Some(s) = lock(&i.source).as_mut() {
                    s.set_gain(db).map_err(|e| e.to_string())?;
                }
                i.save();
                Ok(json!({}))
            }
            Cmd::Rate { hz } => {
                let was = i.running.load(Ordering::SeqCst);
                if was {
                    stop(i);
                }
                lock(&i.cfg).rate = hz;
                i.save();
                if was {
                    start(i)?;
                }
                Ok(json!({}))
            }
            Cmd::Fft { size } => {
                let size = size.clamp(256, 16384).next_power_of_two().min(16384);
                lock(&i.cfg).fft_size = size;
                i.bump_params(|p| p.fft_size = size);
                i.save();
                Ok(json!({}))
            }
            Cmd::Squelch { db } => {
                lock(&i.cfg).squelch = db;
                i.bump_params(|p| p.squelch = db);
                i.save();
                Ok(json!({}))
            }
            Cmd::Station { lat, lon, alt_m } => {
                set_station(i, lat, lon, alt_m);
                Ok(json!({}))
            }
            Cmd::Locator { loc } => {
                let (lat, lon) = locator::to_lat_lon(&loc).ok_or("bad locator")?;
                set_station(i, lat, lon, None);
                Ok(json!({ "lat": lat, "lon": lon }))
            }
            Cmd::Gps { lat, lon, alt_m } => {
                let (follow, st) = {
                    let c = lock(&i.cfg);
                    (c.follow_gps, c.station)
                };
                // Recalculate only for a real move (200 m, 50 m height)
                let dlat = (lat - st.lat) * 111.0;
                let dlon = (lon - st.lon) * 111.0 * lat.to_radians().cos();
                if follow && ((dlat * dlat + dlon * dlon).sqrt() > 0.2 || (alt_m - st.alt_m).abs() > 50.0) {
                    set_station(i, lat, lon, Some(alt_m));
                }
                Ok(json!({}))
            }
            Cmd::FollowGps { on } => {
                lock(&i.cfg).follow_gps = on;
                i.save();
                Ok(json!({}))
            }
            Cmd::Filter { filter } => {
                lock(&i.cfg).filter = filter;
                i.save();
                let _ = lock(&i.work).send(Work::Recalc);
                Ok(json!({}))
            }
            Cmd::Favourite { norad, on } => {
                {
                    let mut c = lock(&i.cfg);
                    if on {
                        c.favourites.insert(norad);
                    } else {
                        c.favourites.remove(&norad);
                    }
                }
                i.save();
                let _ = lock(&i.work).send(Work::Recalc);
                Ok(json!({}))
            }
            Cmd::SatUpdate => {
                let _ = lock(&i.work).send(Work::Download);
                Ok(json!({}))
            }
            Cmd::SatSelect { norad, tx } => {
                select(i, norad, tx);
                Ok(json!({}))
            }
            Cmd::SatTrack { on } => {
                set_tracking(i, on);
                Ok(json!({}))
            }
            Cmd::SatResetOffset => {
                lock(&i.sat).tracker.reset();
                Ok(json!({}))
            }
            Cmd::Simulate => {
                simulate(i);
                Ok(json!({}))
            }
            Cmd::RealTime => {
                *lock(&i.time_shift) = (0.0, 0.0);
                lock(&i.sat).tracker.reset();
                let _ = lock(&i.work).send(Work::Recalc);
                Ok(json!({}))
            }
            Cmd::StepHz { hz } => {
                let hz = hz.clamp(1.0, 1000.0);
                lock(&i.cfg).step_hz = hz;
                lock(&i.sat).tracker.step_hz = hz;
                i.save();
                Ok(json!({}))
            }
            Cmd::CenterTuning { on } => {
                lock(&i.cfg).center_tuning = on;
                if on {
                    let v = lock(&i.cfg).vfo_hz;
                    tune(i, v)?;
                }
                i.save();
                Ok(json!({}))
            }
            Cmd::AutoDemod { on } => {
                lock(&i.cfg).auto_demod = on;
                i.save();
                Ok(json!({}))
            }
            Cmd::SatList => Ok(sat_list(i)),
            Cmd::SatDetail => Ok(sat_detail(i)),
            Cmd::Status => Ok(status(i)),
            Cmd::Messages => Ok(json!({ "messages": lock(&i.messages).iter().cloned().collect::<Vec<_>>() })),
            Cmd::Config => serde_json::to_value(lock(&i.cfg).clone()).map_err(|e| e.to_string()),
            Cmd::UsbAttach { fd, vid, pid, product } => {
                let blog_v4 = product.as_deref().map(|p| p.contains("Blog V4")).unwrap_or(false);
                let kind = if (vid, pid) == sh_sources::usb::ESP32S3_JTAG {
                    "esp"
                } else if sh_sources::usb::is_rtl(vid, pid) {
                    "rtl"
                } else {
                    return Err(format!("unknown USB device {vid:04x}:{pid:04x}"));
                };
                {
                    let mut d = lock(&i.devices);
                    d.retain(|x| x.kind != kind);
                    d.push(UsbDev { kind, vid, pid, fd, blog_v4 });
                }
                i.note(format!("USB: {kind} {vid:04x}:{pid:04x} {} plugged in", product.unwrap_or_default()));
                if lock(&i.cfg).usb_autostart {
                    switch_source(i, kind)?;
                }
                Ok(json!({ "source": kind }))
            }
            Cmd::UsbDetach { vid, pid } => {
                let gone: Vec<UsbDev> = {
                    let mut d = lock(&i.devices);
                    let g = d.iter().filter(|x| x.vid == vid && x.pid == pid).cloned().collect();
                    d.retain(|x| !(x.vid == vid && x.pid == pid));
                    g
                };
                let cur = lock(&i.cfg).source.clone();
                if gone.iter().any(|g| g.kind == cur) {
                    i.note(format!("USB: {cur} unplugged"));
                    stop(i);
                    if lock(&i.cfg).demo_when_no_device {
                        switch_source(i, "demo")?;
                    }
                }
                Ok(json!({}))
            }
            Cmd::Source { id } => {
                switch_source(i, &id)?;
                Ok(json!({}))
            }
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        stop(&self.inner);
        self.inner.quit.store(true, Ordering::SeqCst);
        // Wake the worker: a closed channel ends its loop
        let (dead, _) = channel();
        *lock(&self.inner.work) = dead;
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Source and tuning

fn usb_id(i: &Inner, kind: &str) -> sh_sources::usb::UsbId {
    let d = lock(&i.devices).iter().find(|d| d.kind == kind).copied();
    match (d, kind) {
        (Some(d), _) => sh_sources::usb::UsbId { vid: d.vid, pid: d.pid, fd: d.fd, blog_v4: d.blog_v4 },
        // Desktop: found by vendor / product ID
        (None, "esp") => sh_sources::usb::UsbId { vid: 0x303a, pid: 0x1001, fd: None, blog_v4: false },
        (None, _) => sh_sources::usb::UsbId { vid: 0x0bda, pid: 0x2838, fd: None, blog_v4: false },
    }
}

fn make_source(i: &Inner, id: &str) -> Result<Box<dyn Source>, String> {
    match id {
        "demo" => Ok(Box::new(DemoSource::new(i.sat_signal.clone()))),
        "rtl" => Ok(Box::new(sh_sources::rtl::RtlSource::new(usb_id(i, "rtl")))),
        "esp" => Ok(Box::new(sh_sources::esp::EspSource::new(usb_id(i, "esp")))),
        other => Err(format!("unknown source {other}")),
    }
}

/// Change the source, keeping each source's own last frequency (the ESP-SDR only covers
/// 2204-2804 MHz), and start it.
fn switch_source(i: &Arc<Inner>, id: &str) -> Result<(), String> {
    let info = make_source(i, id)?.info();
    stop(i);
    {
        let mut c = lock(&i.cfg);
        let old = c.source.clone();
        let v = c.vfo_hz;
        c.freqs.insert(old, v);
        let def = if id == "esp" { 2_450_000_000.0 } else { 100_000_000.0 };
        let mut f = c.freqs.get(id).copied().unwrap_or(def);
        if !(info.min_hz..=info.max_hz).contains(&f) {
            f = def;
        }
        c.source = id.to_string();
        c.vfo_hz = f;
        c.center_hz = f - 0.2 * if info.sample_rates.contains(&c.rate) { c.rate } else { info.sample_rates[0] };
        c.center_hz = c.center_hz.clamp(info.min_hz, info.max_hz);
        if !info.sample_rates.contains(&c.rate) {
            c.rate = info.sample_rates[0];
        }
    }
    let vfo = lock(&i.cfg).vfo_hz;
    i.bump_params(|p| p.vfo_hz = vfo);
    i.save();
    start(i)
}

fn start(i: &Arc<Inner>) -> Result<(), String> {
    if i.running.load(Ordering::SeqCst) {
        return Ok(());
    }
    let c = lock(&i.cfg).clone();
    let mut src = make_source(i, &c.source)?;
    let info = src.info();
    let rate = if info.sample_rates.contains(&c.rate) { c.rate } else { info.sample_rates[0] };
    src.set_sample_rate(rate).map_err(|e| e.to_string())?;
    let center = c.center_hz.clamp(info.min_hz, info.max_hz);
    src.set_frequency(center).map_err(|e| e.to_string())?;
    src.set_gain(c.gain).map_err(|e| e.to_string())?;
    src.set_ppm(c.ppm).map_err(|e| e.to_string())?;
    if src.has_bias_tee() {
        src.set_bias_tee(c.bias_tee).map_err(|e| e.to_string())?;
    }
    let (sink, rx, dropped) = Sink::new(32);
    src.start(sink).map_err(|e| e.to_string())?;
    {
        let mut cc = lock(&i.cfg);
        cc.rate = rate;
        cc.center_hz = center;
    }
    *lock(&i.source) = Some(src);
    *lock(&i.dropped) = Some(dropped);
    i.running.store(true, Ordering::SeqCst);
    let ii = i.clone();
    let t = std::thread::Builder::new().name("dsp".into()).spawn(move || dsp(ii, rx)).map_err(|e| e.to_string())?;
    *lock(&i.dsp_thread) = Some(t);
    // The VFO must be inside the new span
    let vfo = lock(&i.cfg).vfo_hz;
    tune(i, vfo)?;
    i.note(format!("started {} at {:.3} MHz, {:.3} MS/s", info.name, center / 1e6, rate / 1e6));
    Ok(())
}

fn stop(i: &Inner) {
    i.running.store(false, Ordering::SeqCst);
    if let Some(mut s) = lock(&i.source).take() {
        s.stop();
    }
    if let Some(t) = lock(&i.dsp_thread).take() {
        let _ = t.join();
    }
    lock(&i.audio).clear();
}

fn set_center(i: &Inner, hz: f64) -> Result<f64, String> {
    let actual = match lock(&i.source).as_mut() {
        Some(s) => s.set_frequency(hz).map_err(|e| e.to_string())?,
        None => hz,
    };
    lock(&i.cfg).center_hz = actual;
    Ok(actual)
}

/// Tune the VFO. If it would leave the usable span, move the hardware so the signal sits a
/// little away from the DC spike.
fn tune(i: &Inner, hz: f64) -> Result<(), String> {
    let (center, rate, bw, centered) = {
        let c = lock(&i.cfg);
        (c.center_hz, c.rate, c.bw, c.center_tuning)
    };
    if centered {
        let c = set_center(i, hz)?;
        lock(&i.cfg).vfo_hz = c;
        i.bump_params(|p| p.vfo_hz = c);
        return Ok(());
    }
    let edge = rate * 0.45 - bw / 2.0;
    if (hz - center).abs() > edge.max(rate * 0.05) {
        set_center(i, hz - 0.2 * rate)?;
    }
    lock(&i.cfg).vfo_hz = hz;
    i.bump_params(|p| p.vfo_hz = hz);
    Ok(())
}

fn dsp(i: Arc<Inner>, rx: Receiver<sh_sources::Block>) {
    let mut ver = 0;
    let mut p = lock(&i.params).clone();
    let mut spec: Option<(Spectrum, f64, usize)> = None;
    let mut demod: Option<(Demodulator, f64, Mode, f64)> = None;
    let mut audio = Vec::with_capacity(4096);
    while i.running.load(Ordering::SeqCst) {
        let b = match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(b) => b,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => break,
        };
        {
            let lp = lock(&i.params);
            if lp.version != ver {
                ver = lp.version;
                p = lp.clone();
            }
        }
        if !matches!(&spec, Some((_, r, n)) if *r == b.rate && *n == p.fft_size) {
            spec = Some((Spectrum::new(p.fft_size, b.rate, p.fps, 0.4), b.rate, p.fft_size));
        }
        let offset = p.vfo_hz - b.center_hz;
        if !matches!(&demod, Some((_, r, m, bw)) if *r == b.rate && *m == p.mode && *bw == p.bw) {
            demod = Some((Demodulator::new(p.mode, b.rate, offset, p.bw), b.rate, p.mode, p.bw));
        }
        if let Some((s, _, _)) = spec.as_mut() {
            let mut new_rows = Vec::new();
            s.push(&b.samples, |f| new_rows.push(f.to_vec()));
            if !new_rows.is_empty() {
                let mut q = lock(&i.rows);
                for r in new_rows {
                    q.push_back((b.center_hz, b.rate, r));
                }
                while q.len() > ROWS_MAX {
                    q.pop_front();
                }
            }
        }
        if let Some((d, _, _, _)) = demod.as_mut() {
            if (d.offset() - offset).abs() > 0.5 {
                d.set_offset(offset);
            }
            d.squelch_db = p.squelch;
            audio.clear();
            d.process(&b.samples, &mut audio);
            let mut q = lock(&i.audio);
            q.extend(audio.iter());
            while q.len() > AUDIO_MAX {
                q.pop_front();
            }
            let mut o = lock(&i.out);
            o.level_db = d.level_db();
            o.bins = p.fft_size;
            o.center_hz = b.center_hz;
            o.rate = b.rate;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Satellites

fn set_station(i: &Inner, lat: f64, lon: f64, alt_m: Option<f64>) {
    {
        let mut c = lock(&i.cfg);
        c.station.lat = lat.clamp(-90.0, 90.0);
        c.station.lon = lon.clamp(-180.0, 180.0);
        if let Some(a) = alt_m {
            c.station.alt_m = a;
        }
    }
    i.save();
    let _ = lock(&i.work).send(Work::Recalc);
}

fn select(i: &Inner, norad: u64, tx: Option<String>) {
    let (sats, filter) = (lock(&i.sat).sats.clone(), lock(&i.cfg).filter.clone());
    let Some(s) = sats.iter().find(|s| s.norad == norad) else { return };
    let tx = tx.or_else(|| s.tx.iter().find(|t| filter.tx_matches(t)).or(s.tx.first()).map(|t| t.uuid.clone()));
    {
        let mut c = lock(&i.cfg);
        c.selected = Some(norad);
        c.selected_tx = tx;
    }
    {
        let mut st = lock(&i.sat);
        st.sel_pass = None;
        st.track = None;
        st.tracker.reset();
    }
    i.save();
    let _ = lock(&i.work).send(Work::Recalc);
    set_tracking(i, true);
}

fn set_tracking(i: &Inner, on: bool) {
    let auto = lock(&i.cfg).auto_demod;
    {
        let mut st = lock(&i.sat);
        st.tracking = on;
        st.tracker.reset();
    }
    if on && auto {
        if let Some(t) = selected_tx(i) {
            let (d, bw) = demod_for(&t);
            let m = match d {
                sh_sat::tracker::Demod::Nfm => Mode::Nfm,
                sh_sat::tracker::Demod::Wfm => Mode::Wfm,
                sh_sat::tracker::Demod::Am => Mode::Am,
                sh_sat::tracker::Demod::Usb => Mode::Usb,
                sh_sat::tracker::Demod::Lsb => Mode::Lsb,
                sh_sat::tracker::Demod::Cw => Mode::Cw,
            };
            {
                let mut c = lock(&i.cfg);
                c.mode = m.name().into();
                c.bw = bw;
            }
            i.bump_params(|p| {
                p.mode = m;
                p.bw = bw;
            });
        }
    }
}

fn selected_tx(i: &Inner) -> Option<sh_sat::Transmitter> {
    let (sel, tx) = {
        let c = lock(&i.cfg);
        (c.selected?, c.selected_tx.clone())
    };
    let sats = lock(&i.sat).sats.clone();
    let s = sats.iter().find(|s| s.norad == sel)?;
    match tx {
        Some(u) => s.tx.iter().find(|t| t.uuid == u).cloned(),
        None => s.tx.first().cloned(),
    }
}

fn simulate(i: &Inner) {
    let pass = lock(&i.sat).sel_pass;
    let Some(p) = pass else { return };
    if p.always {
        return;
    }
    let now = i.sim_now();
    if p.aos <= now {
        return;
    }
    {
        let mut ts = lock(&i.time_shift);
        ts.0 += p.aos - 10.0 - now;
        ts.1 = p.los;
    }
    lock(&i.sat).tracker.reset();
    let _ = lock(&i.work).send(Work::Recalc);
}

fn load_cache(i: &Inner) -> usize {
    let raw = sat::read_cache(&i.dir);
    match sh_sat::db::load(&raw) {
        Ok((sats, stats)) => {
            let n = sats.len();
            let mut s = lock(&i.sat);
            s.sats = Arc::new(sats);
            s.stats = stats;
            s.status.clear();
            n
        }
        Err(e) => {
            lock(&i.sat).status = e.to_string();
            0
        }
    }
}

fn sat_worker(i: Arc<Inner>, rx: Receiver<Work>) {
    load_cache(&i);
    let age = sat::cache_age_hours(&i.dir);
    let days = lock(&i.cfg).update_days;
    let mut pending_download = age.is_none() || (days > 0.0 && age.unwrap_or(0.0) > days * 24.0);
    let mut pending_calc = true;
    while !i.quit.load(Ordering::SeqCst) {
        if pending_download {
            pending_download = false;
            lock(&i.sat).busy = true;
            let ii = i.clone();
            let err = sat::download(&i.dir, |f| lock(&ii.sat).status = format!("Downloading {f} ..."));
            let n = load_cache(&i);
            {
                let mut s = lock(&i.sat);
                s.busy = false;
                if !err.is_empty() {
                    s.status = format!("Update failed: {err}{}", if n > 0 { " (using the old data)" } else { "" });
                }
            }
            i.note(format!("satellite data: {n} satellites{}", if err.is_empty() { String::new() } else { format!(", {err}") }));
            pending_calc = true;
        }
        if pending_calc {
            pending_calc = false;
            recalc(&i);
        }
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(Work::Download) => pending_download = true,
            Ok(Work::Recalc) => pending_calc = true,
            Err(RecvTimeoutError::Timeout) => pending_calc = true, // passes move on: refresh every minute
            Err(RecvTimeoutError::Disconnected) => break,
        }
        // Merge queued requests
        while let Ok(w) = rx.try_recv() {
            match w {
                Work::Download => pending_download = true,
                Work::Recalc => pending_calc = true,
            }
        }
    }
}

fn recalc(i: &Arc<Inner>) {
    let (filter, favs, st, sel) = {
        let c = lock(&i.cfg);
        (c.filter.clone(), c.favourites.clone(), c.station, c.selected)
    };
    let sats = lock(&i.sat).sats.clone();
    let now = i.sim_now();
    let cand = sat::candidates(&sats, &filter, &favs);
    {
        let mut s = lock(&i.sat);
        s.calc_total = cand.len();
        s.calc_done = 0;
    }
    let mut rows = Vec::new();
    let mut last_pub = std::time::Instant::now();
    for (k, (idx, mut r)) in cand.into_iter().enumerate() {
        if i.quit.load(Ordering::SeqCst) {
            return;
        }
        let min_el = if r.fav { 0.0 } else { filter.min_el };
        if let Some(p) = sat::compute_pass(&sats[idx], &st, now, min_el) {
            r.pass = p;
            rows.push(r);
        }
        if last_pub.elapsed() > Duration::from_millis(500) {
            let mut snap = rows.clone();
            sat::sort_rows(&mut snap, now, filter.sort);
            let mut s = lock(&i.sat);
            s.rows = Arc::new(snap);
            s.calc_done = k + 1;
            last_pub = std::time::Instant::now();
        }
    }
    sat::sort_rows(&mut rows, now, filter.sort);
    // Selected satellite: its next pass (any elevation) and the radar track
    let (mut sel_pass, mut trk) = (None, None);
    if let Some(n) = sel {
        if let Some(s) = sats.iter().find(|s| s.norad == n) {
            sel_pass = sat::compute_pass(s, &st, now, 0.0);
            trk = sel_pass.as_ref().map(|p| sat::make_track(s, &st, p));
        }
    }
    let mut s = lock(&i.sat);
    s.rows = Arc::new(rows);
    s.calc_done = s.calc_total;
    s.calc_total = 0;
    s.sel_pass = sel_pass;
    s.track = trk;
    s.last_calc = now;
}

/// 5 Hz: Doppler tracking, the demo source's satellite signal, satellites overhead.
fn control(i: Arc<Inner>) {
    let mut tick = 0u64;
    while !i.quit.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(200));
        tick += 1;
        let now = i.sim_now();
        // Back to real time after a simulated pass
        let sim_los = lock(&i.time_shift).1;
        if sim_los > 0.0 && now > sim_los + 30.0 {
            *lock(&i.time_shift) = (0.0, 0.0);
            let _ = lock(&i.work).send(Work::Recalc);
            i.note("simulated pass over, back to real time");
            continue;
        }
        let (sel, st, vfo) = {
            let c = lock(&i.cfg);
            (c.selected, c.station, c.vfo_hz)
        };
        let sats = lock(&i.sat).sats.clone();
        let sat = sel.and_then(|n| sats.iter().find(|s| s.norad == n));
        let tx = selected_tx(&i);
        let mut signal = None;
        if let (Some(s), Some(t)) = (sat, tx.as_ref()) {
            if let Ok(look) = s.orbit.look(&st, now) {
                let pass = lock(&i.sat).sel_pass;
                let aos_look = if waiting_for_aos(look.el, pass.as_ref(), now) {
                    pass.and_then(|p| s.orbit.look(&st, p.aos).ok())
                } else {
                    None
                };
                let retune = {
                    let mut sst = lock(&i.sat);
                    if sst.tracking {
                        sst.tracker.step(t.centre(), &look, aos_look.as_ref(), vfo)
                    } else {
                        None
                    }
                };
                if let Some(f) = retune {
                    if let Err(e) = tune(&i, f) {
                        i.note(format!("tracking: {e}"));
                        lock(&i.sat).tracking = false;
                    }
                }
                let kind = {
                    let m = t.mode.to_uppercase();
                    if t.is_linear() || m == "USB" || m == "LSB" || m == "SSB" {
                        SignalKind::Linear
                    } else if m == "CW" {
                        SignalKind::Cw
                    } else if m == "FM" || m == "FMN" {
                        SignalKind::Fm
                    } else {
                        SignalKind::Data
                    }
                };
                signal = Some(SatSignal {
                    freq: sh_sat::doppler(t.centre(), look.range_rate_kms),
                    width: if t.is_linear() { t.down_high - t.down_low } else { 0.0 },
                    baud: t.baud,
                    elevation: look.el,
                    range_km: look.range_km,
                    kind,
                });
            }
        }
        *lock(&i.sat_signal) = signal;
        // Satellites overhead now (for the radar), once a second
        if tick % 5 == 0 {
            let rows = lock(&i.sat).rows.clone();
            let mut others = Vec::new();
            for r in rows.iter() {
                if others.len() >= 12 {
                    break;
                }
                if Some(r.norad) == sel || r.pass.aos > now || (r.pass.los < now && !r.pass.always) {
                    continue;
                }
                if let Some(s) = sats.iter().find(|s| s.norad == r.norad) {
                    if let Ok(l) = s.orbit.look(&st, now) {
                        if l.el > 0.0 {
                            others.push(Other { name: r.name.chars().take(10).collect(), az: l.az, el: l.el });
                        }
                    }
                }
            }
            lock(&i.sat).others = others;
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Reports for the UI

fn status(i: &Inner) -> Value {
    let c = lock(&i.cfg).clone();
    let o = lock(&i.out);
    let dropped = lock(&i.dropped).as_ref().map(|d| d.load(Ordering::Relaxed)).unwrap_or(0);
    let (shift, _) = *lock(&i.time_shift);
    json!({
        "running": i.running.load(Ordering::SeqCst),
        "source": c.source,
        "center_hz": c.center_hz,
        "vfo_hz": c.vfo_hz,
        "center_tuning": c.center_tuning,
        "bias_tee": c.bias_tee,
        "ppm": c.ppm,
        "has_bias_tee": c.source == "rtl",
        "rate": c.rate,
        "mode": c.mode,
        "bw": c.bw,
        "gain": c.gain,
        "squelch": c.squelch,
        "level_db": o.level_db,
        "bins": if o.bins > 0 { o.bins } else { c.fft_size },
        "spectrum_center_hz": o.center_hz,
        "spectrum_rate": o.rate,
        "dropped": dropped,
        "source_status": lock(&i.source).as_ref().map(|s| s.status()).unwrap_or_default(),
        "source_name": lock(&i.source).as_ref().map(|s| s.info().name).unwrap_or_default(),
        "devices": lock(&i.devices).iter().map(|d| d.kind).collect::<Vec<_>>(),
        "source_rates": lock(&i.source).as_ref().map(|s| s.info().sample_rates).unwrap_or_default(),
        "source_gains": lock(&i.source).as_ref().map(|s| s.info().gains).unwrap_or_default(),
        "time_shift": shift,
        "station": c.station,
        "locator": locator::from_lat_lon(c.station.lat, c.station.lon),
        "follow_gps": c.follow_gps,
    })
}

fn sat_list(i: &Inner) -> Value {
    let s = lock(&i.sat);
    let c = lock(&i.cfg);
    let now = now_unix() + lock(&i.time_shift).0;
    let rows: Vec<&sat::Row> = s.rows.iter().filter(|r| r.pass.los >= now || r.pass.always).collect();
    let age = sat::cache_age_hours(&i.dir);
    json!({
        "now": now,
        "rows": rows,
        "stats": {
            "satellites": s.stats.satellites, "tles": s.stats.tles, "transmitters": s.stats.transmitters,
            "hungarian": s.stats.hungarian,
        },
        "status": s.status,
        "busy": s.busy,
        "progress": if s.calc_total > 0 { json!([s.calc_done, s.calc_total]) } else { Value::Null },
        "age_hours": age,
        "filter": c.filter,
        "favourites": c.favourites,
        "bands": sh_sat::Band::ALL.iter().map(|b| json!({"id": b, "name": b.name()})).collect::<Vec<_>>(),
    })
}

fn sat_detail(i: &Inner) -> Value {
    let c = lock(&i.cfg).clone();
    let Some(norad) = c.selected else { return json!({ "selected": Value::Null }) };
    let st: Station = c.station;
    let s = lock(&i.sat);
    let Some(sat) = s.sats.iter().find(|x| x.norad == norad) else { return json!({ "selected": Value::Null }) };
    let now = now_unix() + lock(&i.time_shift).0;
    let look = sat.orbit.look(&st, now).ok();
    let tx = c.selected_tx.as_ref().and_then(|u| sat.tx.iter().find(|t| &t.uuid == u)).or(sat.tx.first());
    let txs: Vec<Value> = sat
        .tx
        .iter()
        .map(|t| {
            json!({
                "uuid": t.uuid, "description": t.description, "mode": t.mode, "freq": t.centre(),
                "low": t.down_low, "high": t.down_high, "baud": t.baud, "active": t.active(),
                "matches": c.filter.tx_matches(t),
            })
        })
        .collect();
    let (doppler, rx) = match (look, tx) {
        (Some(l), Some(t)) => {
            let aos = if waiting_for_aos(l.el, s.sel_pass.as_ref(), now) {
                s.sel_pass.and_then(|p| sat.orbit.look(&st, p.aos).ok())
            } else {
                None
            };
            let f = s.tracker.target(t.centre(), &l, aos.as_ref());
            (f - t.centre() - s.tracker.user_offset, f)
        }
        _ => (0.0, 0.0),
    };
    json!({
        "selected": norad,
        "name": sat.name,
        "hungarian": sat.hungarian,
        "favourite": c.favourites.contains(&norad),
        "tx": txs,
        "selected_tx": tx.map(|t| t.uuid.clone()),
        "look": look.map(|l| json!({"az": l.az, "el": l.el, "range_km": l.range_km, "range_rate_kms": l.range_rate_kms})),
        "pass": s.sel_pass,
        "track": s.track,
        "others": s.others,
        "tracking": s.tracking,
        "user_offset": s.tracker.user_offset,
        "doppler_hz": doppler,
        "rx_hz": rx,
        "now": now,
        "simulated": lock(&i.time_shift).0 != 0.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("sathunter-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("sat")).unwrap();
        d
    }

    fn write_iss(d: &Path) {
        std::fs::write(d.join("sat/transmitters.json"), r#"[
          {"uuid":"aprs","description":"APRS","alive":true,"downlink_low":145825000,"mode":"AFSK","baud":1200,"norad_cat_id":25544,"status":"active","service":"Unknown"},
          {"uuid":"lin","description":"Lin","alive":true,"downlink_low":435760000,"downlink_high":435800000,"mode":"USB","norad_cat_id":25544,"status":"active","service":"Amateur"}]"#).unwrap();
        std::fs::write(d.join("sat/tle_satnogs.json"), r#"[{"tle0":"0 ISS","tle1":"1 25544U 98067A   26282.95044904  .00006711  00000-0  13074-3 0  9998","tle2":"2 25544  51.6315  92.1575 0006672 244.0714 115.9587 15.48795780589546","norad_cat_id":25544}]"#).unwrap();
        std::fs::write(d.join("sat/satellites.json"), r#"[{"name":"ISS","norad_cat_id":25544,"status":"in orbit","countries":"RU,US"}]"#).unwrap();
    }

    fn cmd(e: &Engine, j: &str) -> Value {
        serde_json::from_str(&e.command(j)).unwrap()
    }

    #[test]
    fn demo_runs_and_delivers_spectrum_and_audio() {
        let d = tmpdir("demo");
        write_iss(&d);
        // No download in the test: cache is fresh and auto update off
        Config { update_days: 0.0, rate: 1_024_000.0, fft_size: 2048, ..Config::default() }.save(&d.join("config.json"));
        let e = Engine::new(&d);
        assert_eq!(cmd(&e, r#"{"cmd":"start"}"#)["ok"], true);
        std::thread::sleep(Duration::from_millis(600));
        let mut rows = vec![0.0f32; 2048 * 64];
        let mut meta = vec![0.0f64; 128];
        let (n, bins) = e.spectrum_rows(&mut rows, &mut meta);
        assert!(meta[1] > 0.0, "row rate");
        assert!(n > 0 && bins == 2048, "no spectrum rows");
        // Audio keeps coming (a loaded test machine may be slow: wait up to 3 s)
        let mut a = vec![0.0f32; 48_000];
        let mut got = 0;
        for _ in 0..30 {
            got += e.audio(&mut a);
            if got > 10_000 {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(got > 10_000, "audio {got}");
        let st = cmd(&e, r#"{"cmd":"status"}"#);
        assert_eq!(st["running"], true);
        assert_eq!(cmd(&e, r#"{"cmd":"bogus"}"#)["ok"], false);
        cmd(&e, r#"{"cmd":"stop"}"#);
    }

    #[test]
    fn center_tuning_keeps_the_vfo_in_the_middle() {
        let d = tmpdir("ctune");
        write_iss(&d);
        Config { update_days: 0.0, ..Config::default() }.save(&d.join("config.json"));
        let e = Engine::new(&d);
        cmd(&e, r#"{"cmd":"source","id":"demo"}"#);
        assert_eq!(cmd(&e, r#"{"cmd":"center_tuning","on":true}"#)["ok"], true);
        cmd(&e, r#"{"cmd":"tune","hz":101500000}"#);
        let st = cmd(&e, r#"{"cmd":"status"}"#);
        assert_eq!(st["center_hz"], st["vfo_hz"]);
        assert_eq!(st["vfo_hz"].as_f64(), Some(101.5e6));
        cmd(&e, r#"{"cmd":"center","hz":102000000}"#);
        let st = cmd(&e, r#"{"cmd":"status"}"#);
        assert_eq!(st["vfo_hz"].as_f64(), Some(102e6));
        assert_eq!(st["center_tuning"], true);
    }

    #[test]
    fn satellite_selection_tracks_doppler() {
        let d = tmpdir("sat");
        write_iss(&d);
        Config { update_days: 0.0, ..Config::default() }.save(&d.join("config.json"));
        let e = Engine::new(&d);
        // Wait for the pass list
        let mut list = Value::Null;
        for _ in 0..50 {
            list = cmd(&e, r#"{"cmd":"sat_list"}"#);
            if list["rows"].as_array().map(|r| !r.is_empty()).unwrap_or(false) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(list["rows"][0]["norad"], 25544, "{list}");
        cmd(&e, r#"{"cmd":"start"}"#);
        cmd(&e, r#"{"cmd":"sat_select","norad":25544,"tx":"lin"}"#);
        std::thread::sleep(Duration::from_millis(800));
        let det = cmd(&e, r#"{"cmd":"sat_detail"}"#);
        assert_eq!(det["tracking"], true);
        let rx = det["rx_hz"].as_f64().unwrap();
        assert!((rx - 435_780_000.0).abs() < 12_000.0, "rx {rx}");
        let st = cmd(&e, r#"{"cmd":"status"}"#);
        assert_eq!(st["mode"], "USB");
        assert!((st["vfo_hz"].as_f64().unwrap() - rx).abs() < 50.0, "vfo {} rx {rx}", st["vfo_hz"]);
        // Simulated pass: the satellite comes up within seconds
        cmd(&e, r#"{"cmd":"simulate"}"#);
        std::thread::sleep(Duration::from_millis(300));
        let det = cmd(&e, r#"{"cmd":"sat_detail"}"#);
        assert_eq!(det["simulated"], true);
        cmd(&e, r#"{"cmd":"stop"}"#);
    }
}
