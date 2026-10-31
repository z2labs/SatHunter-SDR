//! RF chain simulator: antenna -> LNA -> coax -> R820T -> 8-bit ADC, producing the raw bytes
//! an RTL-SDR delivers, so the real conversion, AGC, spectrum and demodulators can be checked
//! end to end without hardware (the demo source synthesises float samples and skips all that).
//!
//! The model is a classic noise budget (Friis), with a calibration from "dBm at the tuner
//! input" to "dBFS at the ADC" chosen so that the R820T's own noise at 49.6 dB gain sits near
//! -20 dBFS, as on a real RTL-SDR v3 with its input terminated. Absolute levels are therefore
//! approximate (a few dB); differences between settings are what the model is for.

use crate::rtl::{convert, Agc, GAINS};
use sh_dsp::demod::{Demodulator, Mode, AUDIO_RATE};
use sh_dsp::filter::Decimator;
use sh_dsp::spectrum::Spectrum;
use sh_dsp::Complex32;

/// Boltzmann's constant as dBm/Hz/K
const KT0_DBM: f64 = -198.6;

/// A gain stage: gain and noise figure, dB.
#[derive(Debug, Clone, Copy)]
pub struct Stage {
    pub gain_db: f64,
    pub nf_db: f64,
}

/// The receiving chain.
#[derive(Debug, Clone)]
pub struct Chain {
    /// Antenna noise temperature, K (a 70 cm Yagi at the sky with some ground in the pattern)
    pub t_ant_k: f64,
    /// LNA at the antenna (None = antenna straight on the coax)
    pub lna: Option<Stage>,
    pub coax_loss_db: f64,
    /// R820T noise figure
    pub tuner_nf_db: f64,
    /// dBFS at the ADC = dBm at the tuner input + tuner gain + this
    pub cal_db: f64,
    /// ADC + quantisation noise floor, dBFS over the whole band
    pub adc_noise_dbfs: f64,
    /// DC offset in ADC counts (I, Q), IQ gain error dB, IQ phase error degrees
    pub dc: (f64, f64),
    pub iq_gain_db: f64,
    pub iq_phase_deg: f64,
}

fn db(x: f64) -> f64 {
    10.0 * x.log10()
}
fn lin(d: f64) -> f64 {
    10f64.powf(d / 10.0)
}
fn t_of_nf(nf_db: f64) -> f64 {
    290.0 * (lin(nf_db) - 1.0)
}

impl Chain {
    /// 6-element 70 cm Yagi, a mast LNA (0.8 dB NF, 20 dB), 1 dB of coax, RTL-SDR v3.
    /// `lna_powered = false`: the LNA gets no supply (bias tee off) and attenuates ~20 dB.
    pub fn typical(lna_powered: bool) -> Chain {
        Chain {
            t_ant_k: 150.0,
            lna: Some(if lna_powered { Stage { gain_db: 20.0, nf_db: 0.8 } } else { Stage { gain_db: -20.0, nf_db: 20.0 } }),
            coax_loss_db: 1.0,
            tuner_nf_db: 3.5,
            cal_db: 37.1,
            adc_noise_dbfs: -45.0,
            dc: (0.6, -0.4),
            iq_gain_db: 0.2,
            iq_phase_deg: 1.0,
        }
    }

    /// Gain from the antenna terminals to the tuner input, dB.
    pub fn front_gain_db(&self) -> f64 {
        self.lna.map(|s| s.gain_db).unwrap_or(0.0) - self.coax_loss_db
    }

    /// System noise temperature referred to the antenna terminals, K (Friis).
    pub fn t_sys_k(&self) -> f64 {
        let mut t = self.t_ant_k;
        let mut g = 1.0;
        if let Some(s) = self.lna {
            t += t_of_nf(s.nf_db);
            g = lin(s.gain_db);
        }
        // coax: a passive loss L has noise figure L
        t += t_of_nf(self.coax_loss_db) / g;
        g *= lin(-self.coax_loss_db);
        t += t_of_nf(self.tuner_nf_db) / g;
        t
    }

    /// Noise density at the antenna terminals, dBm/Hz.
    pub fn n0_dbm_hz(&self) -> f64 {
        KT0_DBM + db(self.t_sys_k())
    }

    /// Where a power at the antenna terminals lands at the ADC, dBFS.
    pub fn to_dbfs(&self, p_dbm: f64, tuner_gain_db: f64) -> f64 {
        p_dbm + self.front_gain_db() + tuner_gain_db + self.cal_db
    }
}

/// A carrier at the antenna: power dBm, offset from the tuned centre Hz. `keyed`: CW keying
/// pattern (None = steady carrier).
#[derive(Debug, Clone, Copy)]
pub struct Carrier {
    pub p_dbm: f64,
    pub offset_hz: f64,
}

/// Small deterministic Gaussian generator (xorshift + Box-Muller).
pub struct Rng(u64, Option<f64>);

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng(seed | 1, None)
    }
    fn u(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    pub fn gauss(&mut self) -> f64 {
        if let Some(v) = self.1.take() {
            return v;
        }
        let (a, b) = (self.u(), self.u());
        let r = (-2.0 * a.ln()).sqrt();
        let t = 2.0 * std::f64::consts::PI * b;
        self.1 = Some(r * t.sin());
        r * t.cos()
    }
}

/// `n` complex samples as raw RTL bytes (2n bytes).
pub fn render_u8(chain: &Chain, gain_db: f64, fs: f64, carrier: Option<Carrier>, n: usize, phase0: &mut f64, rng: &mut Rng) -> Vec<u8> {
    // Noise from the chain over the whole sampled band, plus the ADC's own
    let n_chain = lin(chain.to_dbfs(chain.n0_dbm_hz() + db(fs), gain_db));
    let n_adc = lin(chain.adc_noise_dbfs);
    let sigma = ((n_chain + n_adc) / 2.0).sqrt(); // per component
    let (amp, w) = match carrier {
        Some(c) => (lin(chain.to_dbfs(c.p_dbm, gain_db)).sqrt(), 2.0 * std::f64::consts::PI * c.offset_hz / fs),
        None => (0.0, 0.0),
    };
    let g_q = 10f64.powf(chain.iq_gain_db / 20.0);
    let ph = chain.iq_phase_deg.to_radians();
    let mut out = Vec::with_capacity(2 * n);
    for _ in 0..n {
        let i = amp * phase0.cos() + sigma * rng.gauss();
        let q = amp * phase0.sin() + sigma * rng.gauss();
        *phase0 = (*phase0 + w).rem_euclid(2.0 * std::f64::consts::PI);
        // IQ imbalance: the Q branch has a gain error and is not quite 90 degrees from I
        let q2 = g_q * (q * ph.cos() + i * ph.sin());
        let to_u8 = |v: f64, dc: f64| (127.5 + dc + 128.0 * v).round().clamp(0.0, 255.0) as u8;
        out.push(to_u8(i, chain.dc.0));
        out.push(to_u8(q2, chain.dc.1));
    }
    out
}

/// The tuner gain the app's automatic gain settles on for this chain (no strong signals).
pub fn agc_gain(chain: &Chain, fs: f64) -> f64 {
    let mut agc = Agc::new();
    let mut g = agc.db() as f64;
    let mut rng = Rng::new(7);
    let mut ph = 0.0;
    let window = 20_000;
    for _ in 0..200 {
        let raw = render_u8(chain, g, fs, None, window, &mut ph, &mut rng);
        if let Some(d) = agc.feed(&raw, window) {
            g = d as f64;
        }
    }
    g
}

/// What the waterfall shows: carrier above the median noise, and how far the noise itself
/// reaches above the median (the 99.9th percentile of the bins), dB. The carrier is visible
/// when the first is clearly above the second.
#[derive(Debug, Clone, Copy)]
pub struct Visibility {
    pub carrier_db: f64,
    pub noise_peaks_db: f64,
    pub bin_hz: f64,
}

impl Visibility {
    pub fn visible(&self) -> bool {
        self.carrier_db > self.noise_peaks_db + 3.0
    }
}

/// Run the app's spectrum over `x` and look at the carrier at `offset_hz`. `legacy`: the
/// spectrum before the averaging fix (one FFT per frame, the rest of the samples skipped).
pub fn spectrum_visibility(x: &[Complex32], fs: f64, fft: usize, offset_hz: f64, legacy: bool) -> Visibility {
    let fps = 30.0;
    let mut frames: Vec<Vec<f32>> = Vec::new();
    if legacy {
        let mut sp = Spectrum::new(fft, fs, fs / fft as f64, 0.4);
        let hop = (fs / fps) as usize;
        for chunk in x.chunks(hop) {
            if chunk.len() >= fft {
                sp.push(&chunk[..fft], |f| frames.push(f.to_vec()));
            }
        }
    } else {
        let mut sp = Spectrum::new(fft, fs, fps, 0.4);
        sp.push(x, |f| frames.push(f.to_vec()));
    }
    let bin_hz = fs / fft as f64;
    let cbin = (fft as f64 / 2.0 + offset_hz / bin_hz).round() as i64;
    // Statistics over the last few frames (what the eye integrates on a waterfall)
    let mut carrier = Vec::new();
    let mut noise = Vec::new();
    for f in frames.iter().rev().take(5) {
        let lo = (cbin - 1).max(0) as usize;
        let hi = (cbin + 1).min(fft as i64 - 1) as usize;
        carrier.push(f[lo..=hi].iter().fold(f32::MIN, |m, v| m.max(*v)) as f64);
        // noise: away from the carrier, DC and the band edges
        for (i, v) in f.iter().enumerate() {
            let i = i as i64;
            if (i - cbin).abs() > 4 && (i - fft as i64 / 2).abs() > 4 && i > fft as i64 / 16 && i < fft as i64 * 15 / 16 {
                noise.push(*v as f64);
            }
        }
    }
    noise.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let med = noise[noise.len() / 2];
    let p999 = noise[(noise.len() as f64 * 0.999) as usize];
    let c = carrier.iter().sum::<f64>() / carrier.len() as f64;
    Visibility { carrier_db: c - med, noise_peaks_db: p999 - med, bin_hz }
}

/// CW audio: tone power in a 100 Hz band around the beat note against the noise in the same
/// width (what the ear hears), dB. `vfo_offset_hz`: where the VFO is relative to the centre.
pub fn cw_audio_snr(x: &[Complex32], fs: f64, vfo_offset_hz: f64, carrier_offset_hz: f64, bw: f64) -> f64 {
    let mut d = Demodulator::new(Mode::Cw, fs, vfo_offset_hz, bw);
    let mut audio = Vec::new();
    d.process(x, &mut audio);
    // Skip the start (filters and AGC settling), FFT the rest
    let n = 16384.min(audio.len() / 2).next_power_of_two() / 2;
    let a = &audio[audio.len() - n..];
    let mut buf: Vec<Complex32> = a
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let w = 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos();
            Complex32::new(*v * w as f32, 0.0)
        })
        .collect();
    let mut planner = rustfft::FftPlanner::new();
    planner.plan_fft_forward(n).process(&mut buf);
    let bin = AUDIO_RATE / n as f64;
    let p: Vec<f64> = buf[..n / 2].iter().map(|v| v.norm_sqr() as f64).collect();
    let beat = (bw / 2.0 + 300.0).max(700.0) + (carrier_offset_hz - vfo_offset_hz);
    if beat.abs() < 50.0 || beat.abs() > AUDIO_RATE / 2.0 - 100.0 {
        return -99.0;
    }
    let k0 = (beat.abs() / bin).round() as usize;
    let half = (50.0 / bin).ceil() as usize;
    let tone: f64 = p[k0.saturating_sub(half)..=(k0 + half).min(p.len() - 1)].iter().sum();
    // Noise density inside the filter's passband (centred on the on-frequency beat note),
    // away from the tone: outside the passband there is no noise to compare with
    let beat0 = (bw / 2.0 + 300.0).max(700.0);
    let (f_lo, f_hi) = ((beat0 - 0.4 * bw).max(100.0), beat0 + 0.4 * bw);
    let mut other: Vec<f64> = p
        .iter()
        .enumerate()
        .filter(|(k, _)| {
            let f = *k as f64 * bin;
            f > f_lo && f < f_hi && (*k as i64 - k0 as i64).unsigned_abs() as usize > 3 * half
        })
        .map(|(_, v)| *v)
        .collect();
    other.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let nd = if other.is_empty() { 1e-30 } else { other[other.len() / 2] };
    let noise = nd * (2 * half + 1) as f64;
    db(tone / noise.max(1e-30)).max(-99.0)
}

/// Raw bytes through the app's RTL conversion, optionally decimated.
pub fn receive(raw: &[u8], fs: f64, decimation: usize) -> (Vec<Complex32>, f64) {
    let mut x = Vec::with_capacity(raw.len() / 2);
    convert(raw, &mut x);
    if decimation <= 1 {
        return (x, fs);
    }
    let mut d = Decimator::new(decimation, fs);
    let mut y = Vec::new();
    for c in x.chunks(16384) {
        let mut part = Vec::new();
        d.process(c, &mut part);
        y.extend(part);
    }
    (y, fs / d.factor() as f64)
}

/// Free-space received power at the antenna terminals, dBm.
pub fn link_dbm(p_tx_dbm: f64, g_tx_dbi: f64, g_rx_dbi: f64, f_hz: f64, range_km: f64, extra_loss_db: f64) -> f64 {
    let fspl = 20.0 * (range_km * 1e3).log10() + 20.0 * f_hz.log10() - 147.55;
    p_tx_dbm + g_tx_dbi + g_rx_dbi - fspl - extra_loss_db
}

/// Highest automatic gain step (dB), for reports.
pub fn max_gain() -> f64 {
    GAINS[GAINS.len() - 1] as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const FS: f64 = 2_400_000.0;

    fn capture(chain: &Chain, p_dbm: f64, offset: f64, secs: f64) -> Vec<u8> {
        let g = agc_gain(chain, FS);
        let mut rng = Rng::new(42);
        let mut ph = 0.0;
        render_u8(chain, g, FS, Some(Carrier { p_dbm, offset_hz: offset }), (FS * secs) as usize, &mut ph, &mut rng)
    }

    #[test]
    fn noise_budget_is_sane() {
        let c = Chain::typical(true);
        let t = c.t_sys_k();
        assert!(t > 200.0 && t < 230.0, "T_sys {t} K");
        // Unpowered LNA: the tuner's noise dominates, T_sys in the 10^4..10^5 K range
        let off = Chain::typical(false);
        assert!(off.t_sys_k() > 30_000.0, "T_sys unpowered {}", off.t_sys_k());
    }

    #[test]
    fn rendered_noise_has_the_budgeted_level() {
        let c = Chain::typical(true);
        let mut rng = Rng::new(1);
        let mut ph = 0.0;
        let raw = render_u8(&c, 30.0, FS, None, 200_000, &mut ph, &mut rng);
        let (x, _) = receive(&raw, FS, 1);
        let p = x.iter().map(|v| v.norm_sqr() as f64).sum::<f64>() / x.len() as f64;
        let want = lin(c.to_dbfs(c.n0_dbm_hz() + db(FS), 30.0)) + lin(c.adc_noise_dbfs);
        assert!((db(p) - db(want)).abs() < 0.5, "{} dBFS vs {}", db(p), db(want));
    }

    #[test]
    fn agc_settles_inside_the_adc_range_with_the_lna() {
        let g = agc_gain(&Chain::typical(true), FS);
        assert!((25.0..=40.3).contains(&g), "agc {g} dB");
    }

    /// The carrier shows on the spectrum at the frequency it is on (sign and bin), via the
    /// raw-byte path. A wrong I/Q order or sign would put it at -offset.
    #[test]
    fn carrier_lands_in_the_right_bin() {
        let c = Chain::typical(true);
        let raw = capture(&c, -100.0, 480_000.0, 0.1);
        let (x, fs) = receive(&raw, FS, 1);
        let v = spectrum_visibility(&x, fs, 2048, 480_000.0, false);
        assert!(v.carrier_db > 30.0, "{v:?}");
        let mirror = spectrum_visibility(&x, fs, 2048, -480_000.0, false);
        assert!(mirror.carrier_db < 20.0, "image {mirror:?}"); // IQ imbalance image, well down
    }

    /// A -135 dBm beacon (weak, low elevation): lost in the old display, visible now.
    #[test]
    fn weak_beacon_old_vs_new_display() {
        let c = Chain::typical(true);
        let off = 480_000.0;
        let raw = capture(&c, -135.0, off, 0.5);
        let (x, fs) = receive(&raw, FS, 1);
        let old = spectrum_visibility(&x, fs, 2048, off, true);
        let new = spectrum_visibility(&x, fs, 8192, off, false);
        assert!(!old.visible(), "old {old:?}");
        assert!(new.visible(), "new {new:?}");
    }

    /// Decimation by 8 with the same FFT size: 8x finer bins, the carrier stands higher.
    #[test]
    fn decimation_sharpens_the_display() {
        let c = Chain::typical(true);
        let raw = capture(&c, -140.0, 60_000.0, 0.5);
        let (x, fs) = receive(&raw, FS, 1);
        let full = spectrum_visibility(&x, fs, 8192, 60_000.0, false);
        let (y, fs8) = receive(&raw, FS, 8);
        assert_eq!(fs8, 300_000.0);
        let dec = spectrum_visibility(&y, fs8, 8192, 60_000.0, false);
        assert!(dec.carrier_db > full.carrier_db + 5.0, "full {full:?} dec {dec:?}");
        assert!(dec.visible(), "{dec:?}");
    }

    /// Unpowered LNA (bias tee off): the beacon is ~20 dB weaker and the tuner's noise rules.
    #[test]
    fn unpowered_lna_loses_the_beacon() {
        let raw_on = capture(&Chain::typical(true), -130.0, 480_000.0, 0.5);
        let raw_off = capture(&Chain::typical(false), -130.0, 480_000.0, 0.5);
        let (a, _) = receive(&raw_on, FS, 1);
        let (b, _) = receive(&raw_off, FS, 1);
        let on = spectrum_visibility(&a, FS, 8192, 480_000.0, false);
        let off = spectrum_visibility(&b, FS, 8192, 480_000.0, false);
        assert!(on.carrier_db > off.carrier_db + 15.0, "on {on:?} off {off:?}");
        assert!(!off.visible(), "{off:?}");
    }

    /// The old 500 Hz CW filter misses a beacon 600 Hz off (crystal + TX offset); 2 kHz hears it.
    #[test]
    fn cw_filter_width_and_frequency_error() {
        let c = Chain::typical(true);
        let raw = capture(&c, -125.0, 480_600.0, 0.6);
        let (x, fs) = receive(&raw, FS, 1);
        let narrow = cw_audio_snr(&x, fs, 480_000.0, 480_600.0, 500.0);
        let wide = cw_audio_snr(&x, fs, 480_000.0, 480_600.0, 2000.0);
        assert!(narrow < 6.0, "500 Hz filter, 600 Hz off: {narrow} dB");
        assert!(wide > 15.0, "2 kHz filter: {wide} dB");
    }
}
