//! Demo source: behaves like an RTL-SDR (24-1766 MHz, RTL rates and gain steps, 8-bit samples,
//! DC offset) and synthesises a small world: FM broadcast, airband AM, APRS bursts, a CW beacon,
//! 433.92 MHz ISM bursts, a 70 cm repeater, and the satellite selected in the Cubesat Hunter at
//! its real Doppler frequency. Lets the app run without hardware (classroom, screenshots).

use crate::{Block, Complex32, SatSignal, SharedSatSignal, SignalKind, Sink, Source, SourceError, SourceInfo};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

pub const RATES: [f64; 4] = [2_400_000.0, 2_048_000.0, 1_024_000.0, 250_000.0];
pub const GAINS: [f32; 29] = [
    0.0, 0.9, 1.4, 2.7, 3.7, 7.7, 8.7, 12.5, 14.4, 15.7, 16.6, 19.7, 20.7, 22.9, 25.4, 28.0, 29.7, 32.8, 33.8, 36.4, 37.2,
    38.6, 40.2, 42.1, 43.4, 43.9, 44.5, 48.0, 49.6,
];

/// 4096-entry sine table with a 32-bit phase accumulator.
struct Lut([f32; 4096 + 1024]);

impl Lut {
    fn new() -> Self {
        let mut t = [0.0f32; 4096 + 1024];
        for (i, v) in t.iter_mut().enumerate() {
            *v = (2.0 * std::f64::consts::PI * i as f64 / 4096.0).sin() as f32;
        }
        Lut(t)
    }
    #[inline]
    fn sin(&self, p: u32) -> f32 {
        self.0[(p >> 20) as usize]
    }
    #[inline]
    fn cos(&self, p: u32) -> f32 {
        self.0[(p >> 20) as usize + 1024]
    }
}

fn inc(hz: f64, fs: f64) -> u32 {
    let r = (hz / fs).rem_euclid(1.0);
    (r * 4_294_967_296.0) as u64 as u32
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    Wfm,
    NfmTones,
    AmTone,
    Cw,
    AfskBurst,
    OokBurst,
    Linear,
}

#[derive(Debug, Clone)]
struct Emitter {
    kind: Kind,
    freq: f64,
    snr_db: f32,
    width: f64,
    baud: f64,
    period: f64,
    text: &'static str,
    carrier: u32,
    audio: u32,
    audio2: u32,
    aux: [u32; 4],
    t: f64,
    rng: u32,
}

impl Emitter {
    fn new(kind: Kind, freq: f64, snr_db: f32) -> Self {
        Emitter {
            kind,
            freq,
            snr_db,
            width: 0.0,
            baud: 1200.0,
            period: 10.0,
            text: "",
            carrier: 0,
            audio: 0,
            audio2: 0,
            aux: [0; 4],
            t: 0.0,
            rng: (freq / 1000.0) as u32 | 1,
        }
    }
    fn rand_bit(&mut self) -> bool {
        // xorshift32
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng & 1 == 1
    }
}

/// Morse key state at time t (dot = 60 ms).
fn morse_key(text: &str, t: f64) -> bool {
    const LETTERS: [&str; 26] = [
        ".-", "-...", "-.-.", "-..", ".", "..-.", "--.", "....", "..", ".---", "-.-", ".-..", "--", "-.", "---", ".--.",
        "--.-", ".-.", "...", "-", "..-", "...-", ".--", "-..-", "-.--", "--..",
    ];
    const DIGITS: [&str; 10] = ["-----", ".----", "..---", "...--", "....-", ".....", "-....", "--...", "---..", "----."];
    let code = |c: char| -> Option<&'static str> {
        match c {
            'A'..='Z' => Some(LETTERS[c as usize - 'A' as usize]),
            '0'..='9' => Some(DIGITS[c as usize - '0' as usize]),
            _ => None,
        }
    };
    // Key pattern in dot units
    let mut pat: Vec<bool> = Vec::new();
    for c in text.chars() {
        match code(c) {
            None => pat.extend([false; 4]),
            Some(cd) => {
                for d in cd.chars() {
                    pat.extend(std::iter::repeat(true).take(if d == '.' { 1 } else { 3 }));
                    pat.push(false);
                }
                pat.extend([false; 2]);
            }
        }
    }
    if pat.is_empty() {
        return false;
    }
    pat[((t / 0.06) as usize) % pat.len()]
}

struct World {
    emitters: Vec<Emitter>,
    sat: Option<Emitter>,
    lut: Lut,
    noise: Vec<Complex32>,
    noise_pos: usize,
}

impl World {
    fn new() -> Self {
        let mut e = vec![
            Emitter::new(Kind::Wfm, 88.6e6, 38.0),
            Emitter::new(Kind::Wfm, 94.8e6, 32.0),
            Emitter::new(Kind::Wfm, 100.0e6, 44.0),
            Emitter::new(Kind::Wfm, 104.3e6, 28.0),
            Emitter::new(Kind::AmTone, 127.5e6, 22.0),
        ];
        let mut aprs = Emitter::new(Kind::AfskBurst, 144.800e6, 26.0);
        aprs.period = 12.0;
        e.push(aprs);
        let mut cw = Emitter::new(Kind::Cw, 432.400e6, 18.0);
        cw.text = "VVV DE HG0DEMO DEMO BEACON   ";
        e.push(cw);
        let mut ook = Emitter::new(Kind::OokBurst, 433.920e6, 30.0);
        ook.period = 7.0;
        e.push(ook);
        let mut rpt = Emitter::new(Kind::NfmTones, 439.300e6, 24.0);
        rpt.period = 20.0;
        e.push(rpt);
        // Gaussian noise table (Box-Muller from a fixed seed)
        let mut s: u32 = 42;
        let mut uni = || {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            (s as f64 + 1.0) / 4_294_967_297.0
        };
        let noise = (0..1 << 16)
            .map(|_| {
                let (u1, u2) = (uni(), uni());
                let r = (-2.0 * u1.ln()).sqrt();
                let a = 2.0 * std::f64::consts::PI * u2;
                Complex32::new((r * a.cos()) as f32, (r * a.sin()) as f32)
            })
            .collect();
        World { emitters: e, sat: None, lut: Lut::new(), noise, noise_pos: 0 }
    }

    fn set_sat(&mut self, sg: Option<SatSignal>) {
        let Some(sg) = sg.filter(|s| s.elevation > 0.0) else {
            self.sat = None;
            return;
        };
        let kind = match sg.kind {
            SignalKind::Cw => Kind::Cw,
            SignalKind::Data => Kind::AfskBurst,
            SignalKind::Linear => Kind::Linear,
            SignalKind::Fm => Kind::NfmTones,
        };
        let keep = matches!(&self.sat, Some(s) if s.kind == kind && (s.freq - sg.freq).abs() < 100e3);
        if !keep {
            let mut e = Emitter::new(kind, sg.freq, 20.0);
            e.text = "CQ CQ DE DEMOSAT   ";
            e.period = 4.0;
            self.sat = Some(e);
        }
        if let Some(e) = self.sat.as_mut() {
            e.freq = sg.freq;
            e.width = sg.width;
            e.baud = if sg.baud > 0.0 { sg.baud } else { 1200.0 };
            // Link budget: ~32 dB overhead at 500 km, weaker with range, fading near the horizon
            let r = sg.range_km.max(300.0);
            e.snr_db = (32.0 - 20.0 * (r / 500.0).log10() - if sg.elevation < 5.0 { (5.0 - sg.elevation) * 2.0 } else { 0.0 }) as f32;
        }
    }

    fn render(&mut self, f0: f64, fs: f64, gain_db: f32, out: &mut Vec<Complex32>, n: usize) {
        let g = 10f32.powf((gain_db - 30.0) / 20.0);
        let n_amp = 0.012 * g;
        let mut acc = vec![Complex32::new(0.004, -0.003); n]; // RTL-like DC offset
        for (i, a) in acc.iter_mut().enumerate() {
            let v = self.noise[(self.noise_pos + i) & (self.noise.len() - 1)];
            *a += v * n_amp;
        }
        self.noise_pos = (self.noise_pos + n * 7919) & (self.noise.len() - 1);
        if (24e6..=1766e6).contains(&f0) {
            let lut = &self.lut;
            for e in self.emitters.iter_mut() {
                synth(lut, e, f0, fs, &mut acc, n_amp);
            }
            if let Some(e) = self.sat.as_mut() {
                synth(lut, e, f0, fs, &mut acc, n_amp);
            }
        }
        // 8-bit quantisation and clipping like the RTL2832U
        out.clear();
        out.extend(acc.iter().map(|v| {
            Complex32::new((v.re.clamp(-1.0, 1.0) * 127.5).round() / 127.5, (v.im.clamp(-1.0, 1.0) * 127.5).round() / 127.5)
        }));
    }
}

fn synth(lut: &Lut, e: &mut Emitter, f0: f64, fs: f64, acc: &mut [Complex32], n_amp: f32) {
    let n = acc.len();
    let off = e.freq - f0;
    let bw_half = match e.kind {
        Kind::Wfm => 100e3,
        Kind::Linear => e.width / 2.0 + 5e3,
        _ => 15e3,
    };
    let dt = 1.0 / fs;
    let t0 = e.t;
    e.t += n as f64 * dt;
    if off.abs() - bw_half > fs / 2.0 {
        return;
    }
    let amp = n_amp * 10f32.powf(e.snr_db / 20.0);
    let c_inc = inc(off, fs);
    let two32 = 4_294_967_296.0;
    let mut put = |i: usize, ph: u32, a: f32| {
        acc[i].re += a * lut.cos(ph);
        acc[i].im += a * lut.sin(ph);
    };
    match e.kind {
        Kind::Wfm => {
            // Music-like mix: melody + fifth + bass, plus the 19 kHz stereo pilot
            let note = 440.0 * 2f64.powf(((t0 * 2.0) as i64 % 8) as f64 / 12.0);
            let (a1, a2, ab, ap) = (inc(note, fs), inc(note * 1.5, fs), inc(note / 4.0, fs), inc(19_000.0, fs));
            let dev = 75e3 / fs * two32;
            for i in 0..n {
                e.audio = e.audio.wrapping_add(a1);
                e.audio2 = e.audio2.wrapping_add(a2);
                e.aux[0] = e.aux[0].wrapping_add(ab);
                e.aux[1] = e.aux[1].wrapping_add(ap);
                let a = 0.4 * lut.sin(e.audio) + 0.25 * lut.sin(e.audio2) + 0.25 * lut.sin(e.aux[0]) + 0.1 * lut.sin(e.aux[1]);
                e.carrier = e.carrier.wrapping_add(c_inc).wrapping_add((dev * a as f64) as i64 as u32);
                put(i, e.carrier, amp);
            }
        }
        Kind::NfmTones => {
            // Voice-like: syllables of changing pitch with pauses, 3 kHz deviation
            if e.period > 0.0 && t0 % e.period >= e.period * 0.6 {
                return;
            }
            const NOTES: [f64; 8] = [523.0, 587.0, 659.0, 784.0, 880.0, 784.0, 659.0, 587.0];
            let note = NOTES[(t0 * 3.0) as usize % 8];
            let env = 0.5 + 0.5 * (t0 * 2.0 * std::f64::consts::PI * 3.0).sin();
            let ai = inc(note, fs);
            let dev = 3e3 / fs * two32 * env;
            for i in 0..n {
                e.audio = e.audio.wrapping_add(ai);
                e.carrier = e.carrier.wrapping_add(c_inc).wrapping_add((dev * lut.sin(e.audio) as f64) as i64 as u32);
                put(i, e.carrier, amp);
            }
        }
        Kind::AmTone => {
            let ai = inc(1000.0, fs);
            let on = t0 % 8.0 < 5.0;
            for i in 0..n {
                e.audio = e.audio.wrapping_add(ai);
                e.carrier = e.carrier.wrapping_add(c_inc);
                let m = if on { 1.0 + 0.7 * lut.sin(e.audio) } else { 1.0 };
                put(i, e.carrier, amp * 0.6 * m);
            }
        }
        Kind::Cw => {
            let mut i = 0;
            while i < n {
                let key = morse_key(e.text, t0 + i as f64 * dt);
                let m = (i + 64).min(n);
                for k in i..m {
                    e.carrier = e.carrier.wrapping_add(c_inc);
                    if key {
                        put(k, e.carrier, amp);
                    }
                }
                i = m;
            }
        }
        Kind::AfskBurst => {
            // ~0.6 s packet per period: 1200 Bd AFSK (1200/2200 Hz) on FM; faster rates as FSK
            if t0 % e.period > 0.6 {
                return;
            }
            let afsk = e.baud <= 1200.0;
            let bit_t = 1.0 / e.baud.max(300.0);
            let dev = (if afsk { 3e3 } else { e.baud / 2.0 }) / fs * two32;
            let (mark, space) = (inc(1200.0, fs), inc(2200.0, fs));
            let mut bit = (t0 / bit_t) as i64;
            let mut val = e.rand_bit();
            for i in 0..n {
                let t = t0 + i as f64 * dt;
                let b = (t / bit_t) as i64;
                if b != bit {
                    bit = b;
                    val = e.rand_bit();
                }
                let fdev = if afsk {
                    e.audio = e.audio.wrapping_add(if val { mark } else { space });
                    dev * lut.sin(e.audio) as f64
                } else if val {
                    dev
                } else {
                    -dev
                };
                e.carrier = e.carrier.wrapping_add(c_inc).wrapping_add(fdev as i64 as u32);
                put(i, e.carrier, amp);
            }
        }
        Kind::OokBurst => {
            // Weather-station style PWM frames for 0.9 s, then silence
            let ph = t0 % e.period;
            if ph > 0.9 {
                return;
            }
            for i in 0..n {
                let t = ph + i as f64 * dt;
                let bit_idx = (t / 1e-3) as u32;
                let in_bit = (t % 1e-3) / 1e-3;
                let one = (bit_idx.wrapping_mul(2_654_435_761) >> 7) & 1 == 1;
                let key = bit_idx % 50 < 40 && in_bit < if one { 0.7 } else { 0.3 };
                e.carrier = e.carrier.wrapping_add(c_inc);
                if key {
                    put(i, e.carrier, amp);
                }
            }
        }
        Kind::Linear => {
            // A few SSB / CW users spread across the transponder passband
            let w = e.width.max(10e3);
            let users = [-0.3 * w, -0.05 * w, 0.15 * w, 0.35 * w];
            for (u, uo) in users.iter().enumerate() {
                let cw = u == 3;
                let on = if cw { morse_key("CQ DE DEMOSAT  ", t0 + u as f64) } else { (t0 + u as f64 * 1.7) % 6.0 < 4.0 };
                if !on {
                    continue;
                }
                let tone = if cw { 700.0 } else { 600.0 + 800.0 * (((t0 * 4.0) as usize + u) % 2) as f64 };
                let ui = inc(off + uo + tone, fs);
                let a = amp * if cw { 0.8 } else { 0.5 };
                for i in 0..n {
                    e.aux[u] = e.aux[u].wrapping_add(ui);
                    put(i, e.aux[u], a);
                }
            }
        }
    }
}

struct Shared {
    freq: AtomicU64, // f64 bits
    rate: AtomicU64,
    gain: AtomicU64,
    decimation: AtomicU64,
    run: AtomicBool,
}

pub struct DemoSource {
    shared: Arc<Shared>,
    sat: SharedSatSignal,
    thread: Option<JoinHandle<()>>,
}

impl DemoSource {
    pub fn new(sat: SharedSatSignal) -> Self {
        DemoSource {
            shared: Arc::new(Shared {
                freq: AtomicU64::new(100e6f64.to_bits()),
                rate: AtomicU64::new(RATES[0].to_bits()),
                gain: AtomicU64::new(25.4f64.to_bits()),
                decimation: AtomicU64::new(1),
                run: AtomicBool::new(false),
            }),
            sat,
            thread: None,
        }
    }
}

impl Source for DemoSource {
    fn info(&self) -> SourceInfo {
        SourceInfo {
            id: "demo",
            name: "Demo (RTL-SDR)".into(),
            min_hz: 24e6,
            max_hz: 1766e6,
            sample_rates: RATES.to_vec(),
            gains: GAINS.to_vec(),
        }
    }

    fn start(&mut self, sink: Sink) -> Result<(), SourceError> {
        if self.thread.is_some() {
            return Ok(());
        }
        self.shared.run.store(true, Ordering::SeqCst);
        let sh = self.shared.clone();
        let sat = self.sat.clone();
        self.thread = Some(std::thread::Builder::new().name("demo-source".into()).spawn(move || {
            let mut world = World::new();
            let mut next = Instant::now();
            let mut buf = Vec::new();
            let mut dec: Option<(sh_dsp::filter::Decimator, f64)> = None;
            let mut dbuf = Vec::new();
            while sh.run.load(Ordering::SeqCst) {
                let fs = f64::from_bits(sh.rate.load(Ordering::Relaxed));
                let f0 = f64::from_bits(sh.freq.load(Ordering::Relaxed));
                let gain = f64::from_bits(sh.gain.load(Ordering::Relaxed)) as f32;
                let n = ((fs / 100.0) as usize).max(1000); // 10 ms blocks
                world.set_sat(sat.lock().ok().and_then(|g| *g));
                world.render(f0, fs, gain, &mut buf, n);
                let d = sh.decimation.load(Ordering::Relaxed) as usize;
                let (samples, rate) = if d > 1 {
                    if !matches!(&dec, Some((x, r)) if x.factor() == d && *r == fs) {
                        dec = Some((sh_dsp::filter::Decimator::new(d, fs), fs));
                    }
                    let (x, _) = dec.as_mut().unwrap();
                    x.process(&buf, &mut dbuf);
                    (std::mem::take(&mut dbuf), fs / x.factor() as f64)
                } else {
                    (std::mem::take(&mut buf), fs)
                };
                if !sink.push(Block { center_hz: f0, rate, samples }) {
                    break;
                }
                // Real-time pacing
                next += Duration::from_secs_f64(n as f64 / fs);
                let now = Instant::now();
                if now > next + Duration::from_millis(200) {
                    next = now;
                } else if next > now {
                    std::thread::sleep(next - now);
                }
            }
        }).map_err(|e| SourceError::Device(e.to_string()))?);
        Ok(())
    }

    fn stop(&mut self) {
        self.shared.run.store(false, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }

    fn running(&self) -> bool {
        self.thread.is_some()
    }

    fn set_frequency(&mut self, hz: f64) -> Result<f64, SourceError> {
        if !(24e6..=1766e6).contains(&hz) {
            return Err(SourceError::OutOfRange(hz));
        }
        self.shared.freq.store(hz.to_bits(), Ordering::Relaxed);
        Ok(hz)
    }

    fn frequency(&self) -> f64 {
        f64::from_bits(self.shared.freq.load(Ordering::Relaxed))
    }

    fn set_sample_rate(&mut self, fs: f64) -> Result<(), SourceError> {
        if !RATES.contains(&fs) {
            return Err(SourceError::Device(format!("unsupported sample rate {fs}")));
        }
        self.shared.rate.store(fs.to_bits(), Ordering::Relaxed);
        Ok(())
    }

    fn sample_rate(&self) -> f64 {
        f64::from_bits(self.shared.rate.load(Ordering::Relaxed))
    }

    fn set_gain(&mut self, db: Option<f32>) -> Result<(), SourceError> {
        self.shared.gain.store((db.unwrap_or(25.4) as f64).to_bits(), Ordering::Relaxed);
        Ok(())
    }

    fn set_decimation(&mut self, factor: usize) -> Result<usize, SourceError> {
        let f = sh_dsp::filter::Decimator::new(factor, 1.0).factor();
        self.shared.decimation.store(f as u64, Ordering::Relaxed);
        Ok(f)
    }
}

impl Drop for DemoSource {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Shared slot for the satellite signal (Cubesat Hunter writes, demo source reads).
pub fn sat_slot() -> SharedSatSignal {
    Arc::new(Mutex::new(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sh_dsp::spectrum::Spectrum;

    fn peak_near(spec: &[f32], fs: f64, off: f64) -> f32 {
        let n = spec.len();
        let b = (n as f64 / 2.0 + off / fs * n as f64) as usize;
        spec[b.saturating_sub(3)..(b + 4).min(n)].iter().cloned().fold(f32::MIN, f32::max)
    }

    #[test]
    fn fm_station_and_satellite_are_on_the_air() {
        let mut w = World::new();
        let fs = 2_400_000.0;
        let mut buf = Vec::new();
        let mut sp = Spectrum::new(2048, fs, 1000.0, 0.5);
        let mut last = Vec::new();
        for _ in 0..20 {
            w.render(100.0e6, fs, 25.4, &mut buf, 24_000);
            sp.push(&buf, |f| last = f.to_vec());
        }
        let floor = last[100];
        assert!(peak_near(&last, fs, 0.0) > floor + 20.0, "100.0 MHz station missing");

        // Satellite (FM) 300 kHz above the centre at 30 deg elevation
        w.set_sat(Some(SatSignal { freq: 436.3e6, width: 0.0, baud: 0.0, elevation: 30.0, range_km: 800.0, kind: SignalKind::Cw }));
        let mut sp = Spectrum::new(2048, fs, 1000.0, 0.0);
        let mut best = f32::MIN;
        for _ in 0..100 {
            w.render(436.0e6, fs, 25.4, &mut buf, 24_000);
            sp.push(&buf, |f| best = best.max(peak_near(f, fs, 300e3)));
        }
        assert!(best > -60.0, "satellite CW not seen: {best}");
    }

    #[test]
    fn morse_beacon_keys() {
        assert!(morse_key("E", 0.0));
        assert!(!morse_key("E", 0.07));
    }

    #[test]
    fn source_thread_delivers_blocks() {
        let mut s = DemoSource::new(sat_slot());
        let (sink, rx, _) = Sink::new(8);
        s.set_sample_rate(250_000.0).unwrap();
        s.start(sink).unwrap();
        let b = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(b.rate, 250_000.0);
        assert_eq!(b.samples.len(), 2500);
        s.stop();
    }
}
