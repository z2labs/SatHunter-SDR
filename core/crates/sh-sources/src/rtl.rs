//! RTL-SDR source (RTL2832U + R820T/R828D), using the `rtl-sdr-rs` driver (MPL-2.0) on `nusb`.
//! Android: opened from the USB file descriptor; desktop: the first RTL-SDR found.

use crate::usb::UsbId;
use crate::{Block, Complex32, Sink, Source, SourceError, SourceInfo};
use sh_dsp::filter::Decimator;
use rtl_sdr_rs::{AsyncReadConfigChange, AsyncReadEvent, DeviceId, RtlSdr, TunerGain};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

pub const RATES: [f64; 6] = [2_400_000.0, 2_048_000.0, 1_920_000.0, 1_440_000.0, 1_024_000.0, 250_000.0];
/// R820T gain steps (dB)
pub const GAINS: [f32; 29] = [
    0.0, 0.9, 1.4, 2.7, 3.7, 7.7, 8.7, 12.5, 14.4, 15.7, 16.6, 19.7, 20.7, 22.9, 25.4, 28.0, 29.7, 32.8, 33.8, 36.4, 37.2,
    38.6, 40.2, 42.1, 43.4, 43.9, 44.5, 48.0, 49.6,
];

struct Shared {
    freq: AtomicU64,
    rate: AtomicU64,
    gain: Mutex<Option<f32>>,
    pending_freq: AtomicBool,
    pending_gain: AtomicBool,
    bias: AtomicBool,
    pending_bias: AtomicBool,
    /// Crystal error in ppm (f64 bits): the hardware is tuned to f / (1 + ppm), the blocks
    /// carry the true frequency
    ppm: AtomicU64,
    /// 1, 2, 4, 8: the blocks come at rate / decimation (read when the stream starts)
    decimation: AtomicU64,
    run: AtomicBool,
    status: Mutex<String>,
}

pub struct RtlSource {
    id: UsbId,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl RtlSource {
    pub fn new(id: UsbId) -> Self {
        RtlSource {
            id,
            shared: Arc::new(Shared {
                freq: AtomicU64::new(100e6f64.to_bits()),
                rate: AtomicU64::new(RATES[0].to_bits()),
                gain: Mutex::new(None),
                pending_freq: AtomicBool::new(false),
                pending_gain: AtomicBool::new(false),
                bias: AtomicBool::new(false),
                pending_bias: AtomicBool::new(false),
                ppm: AtomicU64::new(0f64.to_bits()),
                decimation: AtomicU64::new(1),
                run: AtomicBool::new(false),
                status: Mutex::new(String::new()),
            }),
            thread: None,
        }
    }
}

/// Frequency to ask the tuner for, so that a crystal `ppm` off puts the LO on `f`.
fn hw_freq(f: f64, ppm: f64) -> u32 {
    (f / (1.0 + ppm * 1e-6)).round() as u32
}

fn true_freq(hw: f64, ppm: f64) -> f64 {
    hw * (1.0 + ppm * 1e-6)
}

/// Raw RTL2832U bytes (I, Q interleaved, offset binary around 127.4) to complex samples.
pub fn convert(raw: &[u8], out: &mut Vec<Complex32>) {
    out.extend(raw.chunks_exact(2).map(|p| Complex32::new((p[0] as f32 - 127.4) / 128.0, (p[1] as f32 - 127.4) / 128.0)));
}

fn manual(db: f32) -> TunerGain {
    TunerGain::Manual((db * 10.0).round() as i32) // tenths of a dB
}

/// Automatic gain done here instead of in the tuner: the R820T/R828D's own AGC lets strong
/// signals (FM broadcast) overload the 8-bit ADC. This one watches the samples and steps the
/// manual gain so the level stays well inside the ADC range: down fast on clipping, up slowly.
pub(crate) struct Agc {
    idx: usize,
    n: usize,
    sum: f64,
    clips: usize,
    settle: usize,
}

impl Agc {
    const START: usize = 14; // 25.4 dB
    /// Highest automatic gain, 40.2 dB: with an LNA in front, more only lifts the noise and
    /// the intermodulation (a weak beacon is not helped by filling the ADC with noise)
    const MAX: usize = 22;

    pub(crate) fn new() -> Agc {
        Agc { idx: Self::START, n: 0, sum: 0.0, clips: 0, settle: 0 }
    }

    pub(crate) fn db(&self) -> f32 {
        GAINS[self.idx]
    }

    /// Feed raw 8-bit I/Q; returns a new gain when it should change. `window`: samples per decision.
    pub(crate) fn feed(&mut self, raw: &[u8], window: usize) -> Option<f32> {
        for p in raw.chunks_exact(2) {
            let (i, q) = (p[0] as f32 - 127.4, p[1] as f32 - 127.4);
            self.sum += (i * i + q * q) as f64;
            if p[0] <= 1 || p[0] >= 254 || p[1] <= 1 || p[1] >= 254 {
                self.clips += 1;
            }
        }
        self.n += raw.len() / 2;
        if self.n < window {
            return None;
        }
        let rms = (self.sum / self.n as f64).sqrt() / 128.0;
        let clip = self.clips as f64 / self.n as f64;
        self.n = 0;
        self.sum = 0.0;
        self.clips = 0;
        if self.settle > 0 {
            self.settle -= 1; // the last change has not reached these samples yet
            return None;
        }
        let old = self.idx;
        if clip > 2e-3 {
            self.idx = self.idx.saturating_sub(3);
        } else if clip > 2e-4 || rms > 0.40 {
            self.idx = self.idx.saturating_sub(1);
        } else if rms < 0.12 && clip == 0.0 && self.idx < Self::MAX {
            self.idx += 1;
        }
        if self.idx != old {
            self.settle = 1;
            Some(self.db())
        } else {
            None
        }
    }
}

fn status_line(tuner: &str, v4: bool, rate: f64, gain: Option<f32>, agc: &Agc, bias: bool) -> String {
    let v4 = if v4 { " (RTL-SDR Blog V4)" } else { "" };
    let g = match gain {
        Some(g) => format!("{g:.1} dB"),
        None => format!("auto {:.1} dB", agc.db()),
    };
    let b = if bias { ", bias tee ON" } else { "" };
    format!("tuner {tuner}{v4}, {:.3} MS/s, gain {g}{b}", rate / 1e6)
}

fn set_status(sh: &Shared, s: String) {
    if let Ok(mut x) = sh.status.lock() {
        *x = s;
    }
}

fn worker(id: UsbId, sh: Arc<Shared>, sink: Sink) {
    let fail = |e: String| {
        log::warn!("RTL-SDR: {e}");
        set_status(&sh, e);
    };
    rtl_sdr_rs::set_blog_v4_hint(id.blog_v4);
    let dev = match id.fd {
        Some(fd) => DeviceId::Fd(fd),
        None => DeviceId::Index(0),
    };
    let mut sdr = match RtlSdr::open(dev) {
        Ok(s) => s,
        Err(e) => return fail(format!("cannot open the RTL-SDR: {e:?}")),
    };
    let rate = f64::from_bits(sh.rate.load(Ordering::SeqCst));
    let mut center = f64::from_bits(sh.freq.load(Ordering::SeqCst));
    let mut ppm = f64::from_bits(sh.ppm.load(Ordering::SeqCst));
    let gain = *sh.gain.lock().unwrap_or_else(|e| e.into_inner());
    let setup = (|| -> Result<(), String> {
        sdr.set_sample_rate(rate as u32).map_err(|e| format!("sample rate: {e:?}"))?;
        sdr.set_center_freq(hw_freq(center, ppm)).map_err(|e| format!("frequency: {e:?}"))?;
        sdr.set_tuner_gain(manual(gain.unwrap_or(GAINS[Agc::START]))).map_err(|e| format!("gain: {e:?}"))?;
        sdr.set_bias_tee(sh.bias.load(Ordering::SeqCst)).map_err(|e| format!("bias tee: {e:?}"))?;
        sdr.reset_buffer().map_err(|e| format!("reset: {e:?}"))?;
        Ok(())
    })();
    if let Err(e) = setup {
        return fail(e);
    }
    let tuner = sdr.get_tuner_id().map(|s| s.to_string()).unwrap_or_else(|_| "?".into());
    let v4 = sdr.is_blog_v4();
    let mut gain = gain;
    let mut agc = Agc::new();
    let window = (rate * 0.1) as usize; // one decision per 100 ms
    set_status(&sh, status_line(&tuner, v4, rate, gain, &agc, sh.bias.load(Ordering::SeqCst)));
    // The pending flags are left as they are: a change made while the device was opening
    // (the engine tunes right after start) is applied below instead of being lost.
    // 32 kB transfers: ~7 ms at 2.4 MS/s, low latency for the waterfall
    let reader = match sdr.into_async_reader(15, 32 * 1024) {
        Ok(r) => r,
        Err(e) => return fail(format!("stream: {e:?}")),
    };
    let ctl = reader.control_handle();
    let mut out: Vec<Complex32> = Vec::with_capacity(16 * 1024);
    let mut raw_iq: Vec<Complex32> = Vec::with_capacity(16 * 1024);
    let mut dec = Decimator::new(sh.decimation.load(Ordering::SeqCst) as usize, rate);
    let out_rate = rate / dec.factor() as f64;
    while sh.run.load(Ordering::SeqCst) {
        if sh.pending_freq.swap(false, Ordering::SeqCst) {
            let f = f64::from_bits(sh.freq.load(Ordering::SeqCst));
            ppm = f64::from_bits(sh.ppm.load(Ordering::SeqCst));
            if let Err(e) = ctl.set_center_freq(hw_freq(f, ppm)) {
                fail(format!("retune: {e:?}"));
            }
        }
        if sh.pending_gain.swap(false, Ordering::SeqCst) {
            gain = *sh.gain.lock().unwrap_or_else(|e| e.into_inner());
            agc = Agc::new();
            let _ = ctl.set_tuner_gain(manual(gain.unwrap_or(agc.db())));
            set_status(&sh, status_line(&tuner, v4, rate, gain, &agc, sh.bias.load(Ordering::SeqCst)));
        }
        if sh.pending_bias.swap(false, Ordering::SeqCst) {
            let on = sh.bias.load(Ordering::SeqCst);
            match ctl.set_bias_tee(on) {
                Ok(_) => set_status(&sh, status_line(&tuner, v4, rate, gain, &agc, on)),
                Err(e) => fail(format!("bias tee: {e:?}")),
            }
        }
        match reader.try_recv() {
            None => std::thread::sleep(Duration::from_millis(2)),
            Some(Err(e)) => {
                fail(format!("USB: {e:?}"));
                break;
            }
            Some(Ok(AsyncReadEvent::Reconfigured { change: AsyncReadConfigChange::CenterFrequency(f), .. })) => center = true_freq(f as f64, ppm),
            Some(Ok(AsyncReadEvent::Samples { data, .. })) => {
                if gain.is_none() {
                    if let Some(db) = agc.feed(&data, window) {
                        let _ = ctl.set_tuner_gain(manual(db));
                        set_status(&sh, status_line(&tuner, v4, rate, gain, &agc, sh.bias.load(Ordering::SeqCst)));
                    }
                }
                out.clear();
                if dec.factor() == 1 {
                    convert(&data, &mut out);
                } else {
                    raw_iq.clear();
                    convert(&data, &mut raw_iq);
                    dec.process(&raw_iq, &mut out);
                }
                if !sink.push(Block { center_hz: center, rate: out_rate, samples: std::mem::take(&mut out) }) {
                    break;
                }
            }
            Some(Ok(_)) => {}
        }
    }
    ctl.stop();
}

impl Source for RtlSource {
    fn info(&self) -> SourceInfo {
        SourceInfo { id: "rtl", name: "RTL-SDR".into(), min_hz: 24e6, max_hz: 1766e6, sample_rates: RATES.to_vec(), gains: GAINS.to_vec() }
    }

    fn start(&mut self, sink: Sink) -> Result<(), SourceError> {
        if self.thread.is_some() {
            return Ok(());
        }
        self.shared.run.store(true, Ordering::SeqCst);
        let (id, sh) = (self.id, self.shared.clone());
        self.thread = Some(std::thread::Builder::new().name("rtl-sdr".into()).spawn(move || worker(id, sh, sink)).map_err(|e| SourceError::Device(e.to_string()))?);
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
        if !(24e6..=1766e6).contains(&hz) {
            return Err(SourceError::OutOfRange(hz));
        }
        self.shared.freq.store(hz.to_bits(), Ordering::SeqCst);
        self.shared.pending_freq.store(true, Ordering::SeqCst);
        Ok(hz)
    }

    fn frequency(&self) -> f64 {
        f64::from_bits(self.shared.freq.load(Ordering::SeqCst))
    }

    fn set_sample_rate(&mut self, fs: f64) -> Result<(), SourceError> {
        if !RATES.contains(&fs) {
            return Err(SourceError::Device(format!("unsupported sample rate {fs}")));
        }
        self.shared.rate.store(fs.to_bits(), Ordering::SeqCst);
        Ok(())
    }

    fn sample_rate(&self) -> f64 {
        f64::from_bits(self.shared.rate.load(Ordering::SeqCst))
    }

    fn set_decimation(&mut self, factor: usize) -> Result<usize, SourceError> {
        let f = Decimator::new(factor, 1.0).factor();
        self.shared.decimation.store(f as u64, Ordering::SeqCst);
        Ok(f)
    }

    fn set_gain(&mut self, db: Option<f32>) -> Result<(), SourceError> {
        *self.shared.gain.lock().unwrap_or_else(|e| e.into_inner()) = db;
        self.shared.pending_gain.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn has_bias_tee(&self) -> bool {
        true
    }

    fn set_bias_tee(&mut self, on: bool) -> Result<(), SourceError> {
        self.shared.bias.store(on, Ordering::SeqCst);
        self.shared.pending_bias.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn set_ppm(&mut self, ppm: f64) -> Result<(), SourceError> {
        self.shared.ppm.store(ppm.clamp(-200.0, 200.0).to_bits(), Ordering::SeqCst);
        self.shared.pending_freq.store(true, Ordering::SeqCst); // retune with the new correction
        Ok(())
    }

    fn status(&self) -> String {
        self.shared.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for RtlSource {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(amp: f32, n: usize) -> Vec<u8> {
        (0..n).flat_map(|k| {
            let ph = k as f32 * 0.1;
            [(127.4 + amp * ph.cos()).round().clamp(0.0, 255.0) as u8, (127.4 + amp * ph.sin()).round().clamp(0.0, 255.0) as u8]
        }).collect()
    }

    #[test]
    fn ppm_correction_round_trips() {
        let f = 435_310_000.0;
        let hw = hw_freq(f, 1.5);
        assert!((hw as f64 - f / (1.0 + 1.5e-6)).abs() <= 0.5);
        assert!((true_freq(hw as f64, 1.5) - f).abs() < 1.0);
        assert_eq!(hw_freq(f, 0.0), 435_310_000);
    }

    #[test]
    fn agc_stops_at_its_ceiling() {
        let mut a = Agc::new();
        let quiet = tone(1.0, 1000);
        for _ in 0..200 {
            a.feed(&quiet, 1000);
        }
        assert_eq!(a.db(), GAINS[Agc::MAX]);
    }

    #[test]
    fn agc_backs_off_on_clipping_and_climbs_on_weak_signals() {
        let mut a = Agc::new();
        let start = a.db();
        // Overload: clipped samples -> gain goes down quickly
        let hot = tone(200.0, 1000);
        let mut changed = None;
        for _ in 0..4 {
            changed = changed.or(a.feed(&hot, 1000));
        }
        assert!(changed.is_some() && a.db() < start);
        // Weak: gain climbs back up
        let mut b = Agc::new();
        let weak = tone(5.0, 1000);
        for _ in 0..20 {
            b.feed(&weak, 1000);
        }
        assert!(b.db() > start);
        // Comfortable level: no change
        let mut c = Agc::new();
        let ok = tone(30.0, 1000);
        for _ in 0..10 {
            assert_eq!(c.feed(&ok, 1000), None);
        }
    }
}
