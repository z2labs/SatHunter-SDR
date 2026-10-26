//! ESP-SDR source: an ESP32-S3 running the ESP-SDR firmware on its USB Serial/JTAG port.
//!
//! Text commands (one per line, the firmware answers "OK" / "ERR ..."):
//! `CAPS`, `VERSION?`, `FREQ <MHz>`, `FOFS <kHz>`, `GAIN MANUAL <0..82>`,
//! `IQS 0 <decimation> <bits> 6 <shift> 2` starts the IQ stream, any byte ends it
//! (the firmware then sends an `IQSEND ...` report), `T <MHz> <kHz>` retunes inside the stream
//! (firmware with IQTUNE). Frames: "IQS1", u32 frame, u64 sample index, u16 samples, u8 bits,
//! u8 flags, u16 decimation, u8 shift, then I/Q (8 or 16 bit), then CRC32 (zlib).
//!
//! The LO sits fs/4 = 4 MHz below the wanted frequency in 1 kHz steps; the chip shifts by +fs/4
//! before its FIR, so LO leakage stays outside the band. The sub-kHz rest is removed here with
//! an NCO. The S3's spectrum is mirrored (RF above the LO comes out negative): conjugated here.

use crate::usb::{CdcPort, UsbId};
use crate::{Block, Complex32, Sink, Source, SourceError, SourceInfo};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const FMIN: f64 = 2204e6;
pub const FMAX: f64 = 2804e6;
pub const RATES: [f64; 3] = [250_000.0, 125_000.0, 62_500.0];

pub fn crc32(p: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *v = c;
        }
        t
    });
    let mut c = 0xFFFF_FFFFu32;
    for &b in p {
        c = t[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

/// 8-bit link at >= 250 kS/s, 16-bit below
pub fn link_bits(rate: f64) -> u32 {
    if rate >= 250_000.0 { 8 } else { 16 }
}

pub fn decimation(rate: f64) -> u32 {
    (16e6 / rate) as u32
}

/// 8-bit link: keep the noise floor a few LSB above 0 (gain index -> output shift)
pub fn out_shift(gain: i32) -> i32 {
    const GI: [i32; 8] = [0, 10, 20, 40, 50, 60, 70, 82];
    const GD: [f64; 8] = [0.0, 10.3, 21.1, 30.2, 41.9, 51.3, 61.3, 73.3];
    let mut db = GD[7];
    for i in 0..7 {
        if gain <= GI[i + 1] {
            db = GD[i] + (GD[i + 1] - GD[i]) * (gain - GI[i]) as f64 / (GI[i + 1] - GI[i]) as f64;
            break;
        }
    }
    ((4.0 - (51.3 - db) / 6.02).round() as i32).clamp(0, 4)
}

/// LO plan: (MHz, kHz, residual Hz the host NCO removes)
pub fn lo_plan(freq: f64, ppm: f64) -> (i64, i64, f64) {
    let f = freq * (1.0 - ppm * 1e-6);
    let fk = ((f / 1e3).round() as i64).clamp((FMIN / 1e3) as i64, (FMAX / 1e3) as i64);
    let lo = fk - 4000;
    (lo / 1000, lo % 1000, f - 4e6 - lo as f64 * 1e3)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Settings {
    freq: f64,
    rate: f64,
    gain: i32,
    ppm: f64,
}

/// IQS1 frame parser with DC removal, conjugation and the fine-tuning NCO.
pub struct Parser {
    buf: Vec<u8>,
    fresh: bool,
    next: Option<u64>,
    dc_i: f64,
    dc_q: f64,
    nco_ph: f64,
    nco_step: f64,
    pub crc_errors: u64,
    pub gaps: u64,
    expect_dec: u32,
}

impl Parser {
    pub fn new(rate: f64, resid: f64) -> Self {
        Parser {
            buf: Vec::new(),
            fresh: true,
            next: None,
            dc_i: 0.0,
            dc_q: 0.0,
            nco_ph: 0.0,
            nco_step: if resid.abs() < 1000.0 { -2.0 * std::f64::consts::PI * resid / rate } else { 0.0 },
            crc_errors: 0,
            gaps: 0,
            expect_dec: decimation(rate),
        }
    }

    pub fn set_residual(&mut self, rate: f64, resid: f64) {
        self.nco_step = if resid.abs() < 1000.0 { -2.0 * std::f64::consts::PI * resid / rate } else { 0.0 };
    }

    pub fn push(&mut self, data: &[u8], out: &mut Vec<Complex32>) {
        self.buf.extend_from_slice(data);
        let mut pos = 0;
        loop {
            let Some(rel) = self.buf[pos..].windows(4).position(|w| w == b"IQS1") else {
                pos = self.buf.len().saturating_sub(3).max(pos);
                break;
            };
            let i = pos + rel;
            if self.buf.len() < i + 24 {
                pos = i;
                break;
            }
            let h = &self.buf[i..];
            let fr = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
            let sidx = u64::from_le_bytes([h[8], h[9], h[10], h[11], h[12], h[13], h[14], h[15]]);
            let ns = u16::from_le_bytes([h[16], h[17]]) as usize;
            let bits = h[18] as usize;
            let dec = u16::from_le_bytes([h[20], h[21]]) as u32;
            let sh = h[23] as i32;
            if (bits != 8 && bits != 16) || ns == 0 || ns > 1024 {
                pos = i + 4;
                continue;
            }
            let len = 24 + ns * bits / 4 + 4;
            if self.buf.len() < i + len {
                pos = i;
                break;
            }
            pos = i + len;
            let crc = u32::from_le_bytes([h[len - 4], h[len - 3], h[len - 2], h[len - 1]]);
            if crc32(&h[..len - 4]) != crc {
                self.crc_errors += 1;
                continue;
            }
            if self.fresh {
                // A new run counts frames from 0: skip the old run's tail
                if fr != 0 || dec != self.expect_dec {
                    continue;
                }
                self.fresh = false;
            }
            if let Some(n) = self.next {
                if sidx != n {
                    self.gaps += 1;
                    let lost = sidx.wrapping_sub(n) as f64;
                    self.nco_ph = (self.nco_ph + self.nco_step * lost) % (2.0 * std::f64::consts::PI);
                }
            }
            self.next = Some(sidx + ns as u64);
            let k = (1i32 << sh) as f32 / 32768.0;
            let p = &h[24..24 + ns * bits / 4];
            let start = out.len();
            let (mut si, mut sq) = (0.0f64, 0.0f64);
            for j in 0..ns {
                let (vi, vq) = if bits == 8 {
                    (p[2 * j] as i8 as f32 * k, p[2 * j + 1] as i8 as f32 * k)
                } else {
                    (i16::from_le_bytes([p[4 * j], p[4 * j + 1]]) as f32 * k, i16::from_le_bytes([p[4 * j + 2], p[4 * j + 3]]) as f32 * k)
                };
                si += vi as f64;
                sq += vq as f64;
                out.push(Complex32::new(vi, vq));
            }
            self.dc_i = 0.98 * self.dc_i + 0.02 * si / ns as f64;
            self.dc_q = 0.98 * self.dc_q + 0.02 * sq / ns as f64;
            for v in out[start..].iter_mut() {
                // DC removal, and conjugate: the S3 puts RF above the LO at negative frequencies
                *v = Complex32::new(v.re - self.dc_i as f32, -(v.im - self.dc_q as f32));
                if self.nco_step != 0.0 {
                    let (s, c) = self.nco_ph.sin_cos();
                    *v *= Complex32::new(c as f32, s as f32);
                    self.nco_ph += self.nco_step;
                }
            }
            self.nco_ph %= 2.0 * std::f64::consts::PI;
        }
        self.buf.drain(..pos.min(self.buf.len()));
    }
}

struct Shared {
    want: Mutex<Settings>,
    dirty: AtomicBool,
    run: AtomicBool,
    info: Mutex<String>,
    error: Mutex<String>,
}

pub struct EspSource {
    id: UsbId,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl EspSource {
    pub fn new(id: UsbId) -> Self {
        EspSource {
            id,
            shared: Arc::new(Shared {
                want: Mutex::new(Settings { freq: 2450e6, rate: RATES[0], gain: 60, ppm: 0.0 }),
                dirty: AtomicBool::new(true),
                run: AtomicBool::new(false),
                info: Mutex::new(String::new()),
                error: Mutex::new(String::new()),
            }),
            thread: None,
        }
    }

    pub fn firmware(&self) -> String {
        self.shared.info.lock().map(|s| s.clone()).unwrap_or_default()
    }

    fn want(&self) -> Settings {
        *self.shared.want.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_want(&self, f: impl FnOnce(&mut Settings)) {
        let mut w = self.shared.want.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut w);
        self.shared.dirty.store(true, Ordering::SeqCst);
    }
}

/// Send a command and wait for a line starting with `expect` (or ERR).
fn command(port: &mut CdcPort, c: &str, expect: &str, timeout: Duration) -> Option<String> {
    port.write(format!("{c}\n").as_bytes()).ok()?;
    let end = Instant::now() + timeout;
    let mut line = Vec::new();
    let mut b = Vec::new();
    while Instant::now() < end {
        b.clear();
        port.read(&mut b, 4096, Duration::from_millis(50)).ok()?;
        for &x in &b {
            if x == b'\n' {
                let s = String::from_utf8_lossy(&line).trim().to_string();
                if s.starts_with("ERR") {
                    return None;
                }
                if s.starts_with(expect) {
                    return Some(s);
                }
                line.clear();
            } else if line.len() < 512 {
                line.push(x);
            }
        }
    }
    None
}

/// End the running stream: any byte stops it, wait for the end report, then a quiet line.
fn stop_stream(port: &mut CdcPort) {
    let _ = port.write(b"\n");
    let end = Instant::now() + Duration::from_secs(2);
    let mut tail = Vec::new();
    let mut b = Vec::new();
    while Instant::now() < end {
        b.clear();
        if port.read(&mut b, 65536, Duration::from_millis(50)).is_err() {
            return;
        }
        tail.extend_from_slice(&b);
        if tail.len() > 8192 {
            tail.drain(..tail.len() - 8192);
        }
        if let Some(p) = tail.windows(6).rposition(|w| w == b"IQSEND") {
            if tail[p..].contains(&b'\n') {
                break;
            }
        }
    }
    std::thread::sleep(Duration::from_millis(50));
    port.flush_input();
}

fn start_stream(port: &mut CdcPort, s: &Settings) -> f64 {
    let (mhz, khz, resid) = lo_plan(s.freq, s.ppm);
    let bits = link_bits(s.rate);
    let sh = if bits == 8 { out_shift(s.gain) } else { 0 };
    command(port, &format!("FREQ {mhz}"), "OK", Duration::from_millis(1500));
    command(port, &format!("FOFS {khz}"), "OK", Duration::from_millis(1500));
    command(port, &format!("GAIN MANUAL {}", s.gain), "OK", Duration::from_millis(1500));
    let _ = port.write(format!("IQS 0 {} {} 6 {} 2\n", decimation(s.rate), bits, sh).as_bytes());
    resid
}

fn worker(id: UsbId, sh: Arc<Shared>, sink: Sink) {
    let fail = |e: String| {
        log::warn!("ESP-SDR: {e}");
        if let Ok(mut x) = sh.error.lock() {
            *x = e;
        }
    };
    let mut port = match CdcPort::open(&id) {
        Ok(p) => p,
        Err(e) => return fail(format!("cannot open the board: {e}")),
    };
    std::thread::sleep(Duration::from_millis(300));
    port.flush_input();
    // Stop a stream left running from an earlier session, wait for a quiet line, then CAPS
    let mut caps = None;
    let mut junk = Vec::new();
    for _ in 0..3 {
        if !sh.run.load(Ordering::SeqCst) {
            return;
        }
        let _ = port.write(b"\n");
        let mut quiet = Instant::now();
        let give_up = Instant::now() + Duration::from_secs(3);
        while Instant::now() < give_up {
            junk.clear();
            if port.read(&mut junk, 65536, Duration::from_millis(50)).is_err() {
                return fail("board unplugged".into());
            }
            if !junk.is_empty() {
                quiet = Instant::now();
            } else if quiet.elapsed() > Duration::from_millis(250) {
                break;
            }
        }
        port.flush_input();
        caps = command(&mut port, "CAPS", "CAPS", Duration::from_millis(1500));
        if caps.is_some() {
            break;
        }
    }
    let Some(caps) = caps else { return fail("no ESP-SDR firmware answering".into()) };
    let caps = format!("{caps} ");
    let mut iq_tune = caps.contains(" IQTUNE ");
    let mut info = "old firmware (no version info)".to_string();
    if caps.contains(" VERSION ") {
        if let Some(v) = command(&mut port, "VERSION?", "VERSION", Duration::from_millis(1500)) {
            let field = |k: &str| -> String {
                let key = format!("\"{k}\":\"");
                v.find(&key).and_then(|a| v[a + key.len()..].split('"').next().map(|s| s.to_string())).unwrap_or_default()
            };
            let rev = field("revision");
            info = format!("{} {}{}", field("build_date"), &rev[..rev.len().min(7)], if rev.contains("-dirty") { "+" } else { "" });
        }
    }
    if let Ok(mut x) = sh.info.lock() {
        *x = format!("{info}{}", if iq_tune { ", smooth tuning" } else { "" });
    }
    log::info!("ESP-SDR: {info}, caps {caps}");

    let mut cur: Option<Settings> = None;
    let mut parser = Parser::new(RATES[0], 0.0);
    let mut last_apply = Instant::now() - Duration::from_secs(1);
    let mut tail: Vec<u8> = Vec::new();
    let mut ends = 0;
    let mut ends_since = Instant::now();
    let mut tune_failures = 0;
    let mut rx = Vec::with_capacity(65536);
    let mut samples = Vec::with_capacity(8192);
    while sh.run.load(Ordering::SeqCst) {
        // Apply new settings: smooth in-stream retune when only the frequency changed
        let min_gap = if iq_tune { Duration::from_millis(100) } else { Duration::from_millis(350) };
        if sh.dirty.load(Ordering::SeqCst) && (cur.is_none() || last_apply.elapsed() >= min_gap) {
            sh.dirty.store(false, Ordering::SeqCst);
            let want = *sh.want.lock().unwrap_or_else(|e| e.into_inner());
            match cur {
                Some(c) if iq_tune && c.rate == want.rate && c.gain == want.gain && c.ppm == want.ppm => {
                    let (mhz, khz, resid) = lo_plan(want.freq, want.ppm);
                    let _ = port.write(format!("T {mhz} {khz}\n").as_bytes());
                    parser.set_residual(want.rate, resid);
                }
                _ => {
                    if cur.is_some() {
                        stop_stream(&mut port);
                    }
                    let resid = start_stream(&mut port, &want);
                    parser = Parser::new(want.rate, resid);
                    tail.clear();
                }
            }
            cur = Some(want);
            last_apply = Instant::now();
        }
        rx.clear();
        if port.read(&mut rx, 65536, Duration::from_millis(50)).is_err() {
            fail("board unplugged".into());
            break;
        }
        if rx.is_empty() {
            continue;
        }
        let Some(c) = cur else { continue };
        // The firmware ended the run on its own (ring overrun): restart instead of going silent
        tail.extend_from_slice(&rx);
        if tail.len() > 512 {
            tail.drain(..tail.len() - 512);
        }
        if let Some(p) = tail.windows(7).rposition(|w| w == b"IQSEND ") {
            if let Some(e) = tail[p..].iter().position(|&x| x == b'\n') {
                let line = String::from_utf8_lossy(&tail[p..p + e]).to_string();
                let nums: Vec<u64> = line[7..].split_whitespace().map_while(|t| t.parse().ok()).collect();
                tail.clear();
                if nums.len() >= 2 {
                    log::warn!("ESP-SDR: firmware ended the stream: {line}");
                    if iq_tune && nums.len() > 13 && nums[13] > 0 && nums[0] != 0 {
                        tune_failures += 1;
                        if tune_failures >= 2 {
                            iq_tune = false;
                            log::warn!("ESP-SDR: in-stream retune off for this session");
                        }
                    }
                    if ends_since.elapsed() > Duration::from_secs(5) {
                        ends_since = Instant::now();
                        ends = 0;
                    }
                    ends += 1;
                    if ends > 5 {
                        fail("the stream keeps ending".into());
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                    port.flush_input();
                    let resid = start_stream(&mut port, &c);
                    parser = Parser::new(c.rate, resid);
                    continue;
                }
            }
        }
        samples.clear();
        parser.push(&rx, &mut samples);
        if !samples.is_empty() && !sink.push(Block { center_hz: c.freq, rate: c.rate, samples: std::mem::take(&mut samples) }) {
            break;
        }
    }
    if cur.is_some() {
        stop_stream(&mut port);
    }
}

impl Source for EspSource {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            id: "esp",
            name: "ESP-SDR (ESP32-S3)".into(),
            min_hz: FMIN,
            max_hz: FMAX,
            sample_rates: RATES.to_vec(),
            gains: vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 82.0],
        }
    }

    fn start(&mut self, sink: Sink) -> Result<(), SourceError> {
        if self.thread.is_some() {
            return Ok(());
        }
        self.shared.run.store(true, Ordering::SeqCst);
        self.shared.dirty.store(true, Ordering::SeqCst);
        let (id, sh) = (self.id, self.shared.clone());
        self.thread = Some(std::thread::Builder::new().name("esp-sdr".into()).spawn(move || worker(id, sh, sink)).map_err(|e| SourceError::Device(e.to_string()))?);
        Ok(())
    }

    fn stop(&mut self) {
        self.shared.run.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn running(&self) -> bool {
        self.thread.as_ref().map(|t| !t.is_finished()).unwrap_or(false)
    }

    fn set_frequency(&mut self, hz: f64) -> Result<f64, SourceError> {
        if !(FMIN..=FMAX).contains(&hz) {
            return Err(SourceError::OutOfRange(hz));
        }
        self.set_want(|w| w.freq = hz);
        Ok(hz)
    }

    fn frequency(&self) -> f64 {
        self.want().freq
    }

    fn set_sample_rate(&mut self, fs: f64) -> Result<(), SourceError> {
        if !RATES.contains(&fs) {
            return Err(SourceError::Device(format!("ESP-SDR rates: 250, 125, 62.5 kS/s (not {fs})")));
        }
        self.set_want(|w| w.rate = fs);
        Ok(())
    }

    fn sample_rate(&self) -> f64 {
        self.want().rate
    }

    fn set_gain(&mut self, db: Option<f32>) -> Result<(), SourceError> {
        let g = db.map(|d| d.round() as i32).unwrap_or(60).clamp(0, 82);
        self.set_want(|w| w.gain = g);
        Ok(())
    }

    fn status(&self) -> String {
        let e = self.shared.error.lock().map(|s| s.clone()).unwrap_or_default();
        if !e.is_empty() { e } else { self.firmware() }
    }
}

impl Drop for EspSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_matches_zlib() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn lo_plan_puts_the_lo_4_mhz_below() {
        let (mhz, khz, resid) = lo_plan(2_450_000_500.0, 0.0);
        assert_eq!((mhz, khz), (2446, 1)); // 2450001 kHz - 4000 kHz
        assert!((resid - -500.0).abs() < 1e-3, "{resid}");
    }

    fn frame(fr: u32, sidx: u64, iq: &[(i8, i8)], dec: u16) -> Vec<u8> {
        let mut f = b"IQS1".to_vec();
        f.extend(fr.to_le_bytes());
        f.extend(sidx.to_le_bytes());
        f.extend((iq.len() as u16).to_le_bytes());
        f.push(8);
        f.push(0);
        f.extend(dec.to_le_bytes());
        f.push(0); // byte 22: reserved
        f.push(0); // byte 23: output shift
        for (i, q) in iq {
            f.push(*i as u8);
            f.push(*q as u8);
        }
        let c = crc32(&f);
        f.extend(c.to_le_bytes());
        f
    }

    #[test]
    fn parses_frames_split_anywhere_and_rejects_bad_crc() {
        let iq: Vec<(i8, i8)> = (0..100).map(|k| ((k % 50) as i8, -((k % 30) as i8))).collect();
        let mut data = b"junk IQSEND 0 1\n".to_vec();
        data.extend(frame(0, 0, &iq, 64));
        let mut bad = frame(1, 100, &iq, 64);
        bad[30] ^= 0xFF;
        data.extend(bad);
        data.extend(frame(2, 200, &iq, 64));
        let mut p = Parser::new(250_000.0, 0.0);
        let mut out = Vec::new();
        for c in data.chunks(7) {
            p.push(c, &mut out);
        }
        assert_eq!(out.len(), 200);
        assert_eq!(p.crc_errors, 1);
        assert_eq!(p.gaps, 1);
        // Conjugated: sample 1 has Q = -1 LSB, it comes out positive
        assert!(out[1].im > 0.0);
    }

    #[test]
    fn skips_the_old_runs_tail() {
        let iq = vec![(1i8, 1i8); 10];
        let mut p = Parser::new(250_000.0, 0.0);
        let mut out = Vec::new();
        p.push(&frame(57, 570, &iq, 64), &mut out); // tail of an earlier run
        assert!(out.is_empty());
        p.push(&frame(0, 0, &iq, 64), &mut out);
        assert_eq!(out.len(), 10);
    }
}
