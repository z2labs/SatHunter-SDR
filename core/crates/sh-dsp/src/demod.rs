//! Demodulators: NFM, WFM, AM, USB, LSB, CW. Output: mono audio at 48 kHz.

use crate::channel::Channel;
use crate::filter::{lowpass, DcBlock, Fir};
use crate::{Complex32, Nco};

pub const AUDIO_RATE: f64 = 48_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Nfm,
    Wfm,
    Am,
    Usb,
    Lsb,
    Cw,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Nfm => "NFM",
            Mode::Wfm => "WFM",
            Mode::Am => "AM",
            Mode::Usb => "USB",
            Mode::Lsb => "LSB",
            Mode::Cw => "CW",
        }
    }
    pub fn parse(s: &str) -> Option<Mode> {
        Some(match s.to_uppercase().as_str() {
            "NFM" | "FM" => Mode::Nfm,
            "WFM" => Mode::Wfm,
            "AM" => Mode::Am,
            "USB" => Mode::Usb,
            "LSB" => Mode::Lsb,
            "CW" => Mode::Cw,
            _ => return None,
        })
    }
    pub fn default_bw(self) -> f64 {
        match self {
            Mode::Nfm => 12_500.0,
            Mode::Wfm => 180_000.0,
            Mode::Am => 10_000.0,
            Mode::Usb | Mode::Lsb => 2_800.0,
            // Wide enough for a beacon a few hundred Hz off (RTL crystal, satellite TX offset)
            Mode::Cw => 2_000.0,
        }
    }
}

/// CW beat note: 700 Hz for a narrow filter; with a wide one, high enough that the whole
/// passband stays above 0 Hz (a carrier 1 kHz below the VFO is still a tone, not DC).
fn cw_beat(bw: f64) -> f64 {
    (bw / 2.0 + 300.0).max(700.0)
}

/// Simple peak AGC for AM / SSB / CW.
#[derive(Debug, Clone)]
struct Agc {
    gain: f32,
    /// per-sample constants, scaled so the timing is the same at any rate (tuned at 48 kHz)
    attack: f32,
    decay: f32,
}

impl Agc {
    fn new(fs: f64) -> Agc {
        let k = (AUDIO_RATE / fs) as f32;
        Agc { gain: 1.0, attack: (0.01 * k).min(0.5), decay: 2e-5 * k }
    }

    fn step(&mut self, x: f32) -> f32 {
        let y = x * self.gain;
        let a = y.abs();
        if a > 0.5 {
            self.gain *= 1.0 - self.attack * (a - 0.5).min(1.0); // fast attack
        } else {
            self.gain *= 1.0 + self.decay; // slow decay
        }
        self.gain = self.gain.clamp(1e-3, 1e6);
        y.clamp(-1.0, 1.0)
    }
}

pub struct Demodulator {
    mode: Mode,
    bw: f64,
    chan: Channel,
    fs_ch: f64,
    last: Complex32,
    fm_gain: f32,
    dc: DcBlock,
    agc: Agc,
    ssb_nco: Nco,
    ssb_back: Nco,
    ssb_fir: Option<crate::filter::FirDecim>,
    audio_fir: Fir,
    post_fir: Option<Fir>,
    post_in: Vec<f32>,
    deemph: f32,
    deemph_a: f32,
    // Resampler state
    pos: f64,
    step: f64,
    prev: f32,
    // Buffers
    tmp: Vec<Complex32>,
    base: Vec<Complex32>,
    ssb_tmp: Vec<Complex32>,
    raw: Vec<f32>,
    filt: Vec<f32>,
    level: f32,
    pub squelch_db: Option<f32>,
}

impl Demodulator {
    pub fn new(mode: Mode, fs_in: f64, offset: f64, bw: f64) -> Self {
        let bw = if bw > 0.0 { bw } else { mode.default_bw() };
        let narrow = matches!(mode, Mode::Usb | Mode::Lsb | Mode::Cw);
        let audio_cut = match mode {
            Mode::Wfm => 15_000.0,
            Mode::Cw => (cw_beat(bw) + bw / 2.0 + 200.0).max(1_200.0),
            Mode::Usb | Mode::Lsb => bw.min(3_500.0),
            _ => 4_000.0,
        };
        // SSB / CW: the sharp sideband filter runs at ~12 kHz instead of 48 kHz (4x fewer taps
        // at 4x fewer samples); the audio is brought to 48 kHz afterwards and cleaned there
        let min_rate = match mode {
            Mode::Wfm => 200_000.0,
            _ if narrow => (2.0 * audio_cut + 4_000.0).max(12_000.0),
            _ => AUDIO_RATE,
        };
        let chan_bw = match mode {
            Mode::Usb | Mode::Lsb => 2.0 * bw + 1000.0,
            Mode::Cw => (bw + 2000.0).max(3000.0),
            _ => bw,
        };
        let chan = Channel::new(fs_in, offset, chan_bw, min_rate);
        let fs_ch = chan.rate();
        let dev = match mode {
            Mode::Wfm => 75_000.0,
            _ => (bw / 2.0 - 3000.0).max(2500.0),
        };
        // Images of the linear interpolation start at fs_ch - audio_cut
        let post_fir = if narrow && fs_ch < AUDIO_RATE * 0.9 {
            let room = (fs_ch - 2.0 * audio_cut).max(1_000.0);
            Some(Fir::new(lowpass(audio_cut + room / 2.0, room, AUDIO_RATE)))
        } else {
            None
        };
        // Sideband filter: shift the wanted half to be centred on 0, filter, shift back
        let (ssb_shift, ssb_fir) = match mode {
            Mode::Usb => (bw / 2.0, Some(crate::filter::FirDecim::new(lowpass(bw / 2.0, 300.0, fs_ch), 1))),
            Mode::Lsb => (-bw / 2.0, Some(crate::filter::FirDecim::new(lowpass(bw / 2.0, 300.0, fs_ch), 1))),
            // CW: narrow filter on the carrier, then a 700 Hz beat note
            Mode::Cw => (0.0, Some(crate::filter::FirDecim::new(lowpass(bw / 2.0, 150.0, fs_ch), 1))),
            _ => (0.0, None),
        };
        let back = if mode == Mode::Cw { -cw_beat(bw) } else { -ssb_shift };
        // WFM de-emphasis 50 us (Europe)
        let tau = 50e-6;
        let deemph_a = (-1.0 / (fs_ch * tau)).exp() as f32;
        Demodulator {
            mode,
            bw,
            chan,
            fs_ch,
            last: Complex32::new(1.0, 0.0),
            fm_gain: (fs_ch / (2.0 * std::f64::consts::PI * dev)) as f32,
            dc: DcBlock::default(),
            agc: Agc::new(fs_ch),
            ssb_nco: Nco::new(ssb_shift, fs_ch),
            ssb_back: Nco::new(back, fs_ch),
            ssb_fir,
            audio_fir: Fir::new(lowpass(audio_cut, audio_cut * 0.3, fs_ch)),
            post_fir,
            post_in: Vec::new(),
            deemph: 0.0,
            deemph_a,
            pos: 0.0,
            step: fs_ch / AUDIO_RATE,
            prev: 0.0,
            tmp: Vec::new(),
            base: Vec::new(),
            ssb_tmp: Vec::new(),
            raw: Vec::new(),
            filt: Vec::new(),
            level: -120.0,
            squelch_db: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn bandwidth(&self) -> f64 {
        self.bw
    }
    pub fn offset(&self) -> f64 {
        self.chan.offset()
    }
    pub fn set_offset(&mut self, offset: f64) {
        self.chan.set_offset(offset);
    }
    /// Channel power, dBFS (smoothed)
    pub fn level_db(&self) -> f32 {
        self.level
    }

    /// Demodulate a block of input samples, appending 48 kHz audio to `audio`.
    pub fn process(&mut self, x: &[Complex32], audio: &mut Vec<f32>) {
        self.base.clear();
        self.chan.process(x, &mut self.tmp, &mut self.base);
        if self.base.is_empty() {
            return;
        }
        let p = self.base.iter().map(|v| v.norm_sqr()).sum::<f32>() / self.base.len() as f32;
        let db = 10.0 * (p + 1e-20).log10();
        self.level = 0.8 * self.level + 0.2 * db;
        let open = self.squelch_db.map(|s| self.level >= s).unwrap_or(true);

        self.raw.clear();
        match self.mode {
            Mode::Nfm | Mode::Wfm => {
                for v in &self.base {
                    let d = (*v * self.last.conj()).arg();
                    self.last = *v;
                    let mut y = d * self.fm_gain;
                    if self.mode == Mode::Wfm {
                        self.deemph = self.deemph_a * self.deemph + (1.0 - self.deemph_a) * y;
                        y = self.deemph;
                    }
                    self.raw.push(y);
                }
            }
            Mode::Am => {
                for v in &self.base {
                    let y = self.dc.step(v.norm());
                    self.raw.push(self.agc.step(y));
                }
            }
            Mode::Usb | Mode::Lsb | Mode::Cw => {
                self.ssb_tmp.clear();
                let shifted: Vec<Complex32> = self.base.iter().map(|v| *v * self.ssb_nco.next()).collect();
                if let Some(f) = self.ssb_fir.as_mut() {
                    f.process(&shifted, &mut self.ssb_tmp);
                }
                for v in &self.ssb_tmp {
                    let y = (*v * self.ssb_back.next()).re;
                    self.raw.push(self.agc.step(y));
                }
            }
        }
        self.filt.clear();
        self.audio_fir.process(&self.raw, &mut self.filt);
        // Linear resampler fs_ch -> 48 kHz (input is already low-passed below 15 kHz)
        self.post_in.clear();
        for &s in &self.filt {
            while self.pos < 1.0 {
                let y = self.prev + (s - self.prev) * self.pos as f32;
                self.post_in.push(y);
                self.pos += self.step;
            }
            self.pos -= 1.0;
            self.prev = s;
        }
        let start = audio.len();
        match self.post_fir.as_mut() {
            Some(f) => f.process(&self.post_in, audio),
            None => audio.extend_from_slice(&self.post_in),
        }
        for y in audio[start..].iter_mut() {
            *y = if open { y.clamp(-1.0, 1.0) } else { 0.0 };
        }
    }

    #[allow(dead_code)]
    fn rate(&self) -> f64 {
        self.fs_ch
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FM-modulated 1 kHz tone, deviation `dev`, carrier at `fc` (relative to centre).
    fn fm(fc: f64, dev: f64, fs: f64, n: usize) -> Vec<Complex32> {
        let mut ph = 0.0f64;
        (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                let f = fc + dev * (2.0 * std::f64::consts::PI * 1000.0 * t).sin();
                ph += 2.0 * std::f64::consts::PI * f / fs;
                Complex32::new(ph.cos() as f32, ph.sin() as f32)
            })
            .collect()
    }

    /// Dominant frequency of a real signal by zero crossings.
    fn freq(x: &[f32], fs: f64) -> f64 {
        let z = x.windows(2).filter(|w| w[0] <= 0.0 && w[1] > 0.0).count();
        z as f64 * fs / x.len() as f64
    }

    #[test]
    fn nfm_recovers_the_tone() {
        let fs = 2_400_000.0;
        let mut d = Demodulator::new(Mode::Nfm, fs, 100_000.0, 12_500.0);
        let mut audio = Vec::new();
        d.process(&fm(100_000.0, 2500.0, fs, 480_000), &mut audio);
        assert!((audio.len() as f64 - 9600.0).abs() < 50.0, "{} samples", audio.len());
        let tail = &audio[audio.len() / 2..];
        assert!((freq(tail, AUDIO_RATE) - 1000.0).abs() < 20.0, "{}", freq(tail, AUDIO_RATE));
        let peak = tail.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.3 && peak <= 1.0, "peak {peak}");
    }

    #[test]
    fn wfm_broadcast_at_2_4_msps() {
        let fs = 2_400_000.0;
        let mut d = Demodulator::new(Mode::Wfm, fs, -480_000.0, 180_000.0);
        let mut audio = Vec::new();
        d.process(&fm(-480_000.0, 50_000.0, fs, 960_000), &mut audio);
        let tail = &audio[audio.len() / 2..];
        assert!((freq(tail, AUDIO_RATE) - 1000.0).abs() < 20.0, "{}", freq(tail, AUDIO_RATE));
        let peak = tail.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.3, "peak {peak}");
    }

    #[test]
    fn usb_gives_the_audio_offset() {
        let fs = 240_000.0;
        // Carrier at 10 kHz, a 1.5 kHz USB tone = a single line at 11.5 kHz
        let mut nco = Nco::new(-11_500.0, fs);
        let x: Vec<Complex32> = (0..240_000).map(|_| nco.next() * 0.1).collect();
        let mut d = Demodulator::new(Mode::Usb, fs, 10_000.0, 2800.0);
        let mut audio = Vec::new();
        d.process(&x, &mut audio);
        let tail = &audio[audio.len() / 2..];
        assert!((freq(tail, AUDIO_RATE) - 1500.0).abs() < 20.0, "{}", freq(tail, AUDIO_RATE));
        // and nothing for LSB
        let mut l = Demodulator::new(Mode::Lsb, fs, 10_000.0, 2800.0);
        let mut a2 = Vec::new();
        l.process(&x, &mut a2);
        let rms_l = (a2[a2.len() / 2..].iter().map(|v| v * v).sum::<f32>() / (a2.len() / 2) as f32).sqrt();
        let rms_u = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
        assert!(rms_l < rms_u * 0.05, "lsb {rms_l} usb {rms_u}");
    }

    #[test]
    fn cw_hears_a_beacon_off_frequency() {
        let fs = 240_000.0;
        for off in [-800.0, 0.0, 800.0] {
            // carrier at 10 kHz + off
            let mut nco = Nco::new(-(10_000.0 + off), fs);
            let x: Vec<Complex32> = (0..240_000).map(|_| nco.next() * 0.1).collect();
            let mut d = Demodulator::new(Mode::Cw, fs, 10_000.0, 0.0);
            let mut audio = Vec::new();
            d.process(&x, &mut audio);
            let tail = &audio[audio.len() / 2..];
            let want = cw_beat(Mode::Cw.default_bw()) + off;
            assert!((freq(tail, AUDIO_RATE) - want).abs() < 20.0, "off {off}: {} Hz", freq(tail, AUDIO_RATE));
            let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
            assert!(rms > 0.1, "off {off}: rms {rms}");
        }
    }
}
