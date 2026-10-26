//! RTL-SDR source (RTL2832U + R820T/R828D), using the `rtl-sdr-rs` driver (MPL-2.0) on `nusb`.
//! Android: opened from the USB file descriptor; desktop: the first RTL-SDR found.

use crate::usb::UsbId;
use crate::{Block, Complex32, Sink, Source, SourceError, SourceInfo};
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
                run: AtomicBool::new(false),
                status: Mutex::new(String::new()),
            }),
            thread: None,
        }
    }
}

fn tuner_gain(g: Option<f32>) -> TunerGain {
    match g {
        None => TunerGain::Auto,
        Some(db) => TunerGain::Manual((db * 10.0).round() as i32), // tenths of a dB
    }
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
    let gain = *sh.gain.lock().unwrap_or_else(|e| e.into_inner());
    let setup = (|| -> Result<(), String> {
        sdr.set_sample_rate(rate as u32).map_err(|e| format!("sample rate: {e:?}"))?;
        sdr.set_center_freq(center as u32).map_err(|e| format!("frequency: {e:?}"))?;
        sdr.set_tuner_gain(tuner_gain(gain)).map_err(|e| format!("gain: {e:?}"))?;
        sdr.reset_buffer().map_err(|e| format!("reset: {e:?}"))?;
        Ok(())
    })();
    if let Err(e) = setup {
        return fail(e);
    }
    let tuner = sdr.get_tuner_id().map(|s| s.to_string()).unwrap_or_else(|_| "?".into());
    let v4 = if sdr.is_blog_v4() { " (RTL-SDR Blog V4)" } else { "" };
    set_status(&sh, format!("tuner {tuner}{v4}, {:.3} MS/s, gain {}", rate / 1e6, gain.map(|g| format!("{g:.1} dB")).unwrap_or("auto".into())));
    sh.pending_freq.store(false, Ordering::SeqCst);
    sh.pending_gain.store(false, Ordering::SeqCst);
    // 32 kB transfers: ~7 ms at 2.4 MS/s, low latency for the waterfall
    let reader = match sdr.into_async_reader(15, 32 * 1024) {
        Ok(r) => r,
        Err(e) => return fail(format!("stream: {e:?}")),
    };
    let ctl = reader.control_handle();
    let mut out: Vec<Complex32> = Vec::with_capacity(16 * 1024);
    while sh.run.load(Ordering::SeqCst) {
        if sh.pending_freq.swap(false, Ordering::SeqCst) {
            let f = f64::from_bits(sh.freq.load(Ordering::SeqCst));
            if let Err(e) = ctl.set_center_freq(f as u32) {
                fail(format!("retune: {e:?}"));
            }
        }
        if sh.pending_gain.swap(false, Ordering::SeqCst) {
            let g = *sh.gain.lock().unwrap_or_else(|e| e.into_inner());
            let _ = ctl.set_tuner_gain(tuner_gain(g));
        }
        match reader.try_recv() {
            None => std::thread::sleep(Duration::from_millis(2)),
            Some(Err(e)) => {
                fail(format!("USB: {e:?}"));
                break;
            }
            Some(Ok(AsyncReadEvent::Reconfigured { change: AsyncReadConfigChange::CenterFrequency(f), .. })) => center = f as f64,
            Some(Ok(AsyncReadEvent::Samples { data, .. })) => {
                out.clear();
                out.extend(data.chunks_exact(2).map(|p| Complex32::new((p[0] as f32 - 127.4) / 128.0, (p[1] as f32 - 127.4) / 128.0)));
                if !sink.push(Block { center_hz: center, rate, samples: std::mem::take(&mut out) }) {
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

    fn set_gain(&mut self, db: Option<f32>) -> Result<(), SourceError> {
        *self.shared.gain.lock().unwrap_or_else(|e| e.into_inner()) = db;
        self.shared.pending_gain.store(true, Ordering::SeqCst);
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
