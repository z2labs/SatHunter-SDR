//! FIR filters: windowed-sinc low-pass design, decimating complex FIR, real FIR, DC blocker.

use crate::Complex32;
use std::f64::consts::PI;

/// Low-pass taps (Blackman window), unity DC gain. `cutoff` and `fs` in Hz.
pub fn lowpass(cutoff: f64, transition: f64, fs: f64) -> Vec<f32> {
    // Blackman: transition width ~ 5.5 / N (normalised)
    let mut n = ((5.5 * fs / transition.max(1.0)).ceil() as usize) | 1;
    n = n.clamp(15, 4095);
    let fc = (cutoff / fs).clamp(1e-6, 0.4999);
    let m = (n - 1) as f64 / 2.0;
    let mut taps: Vec<f64> = (0..n)
        .map(|i| {
            let x = i as f64 - m;
            let sinc = if x == 0.0 { 2.0 * fc } else { (2.0 * PI * fc * x).sin() / (PI * x) };
            let w = 0.42 - 0.5 * (2.0 * PI * i as f64 / (n - 1) as f64).cos() + 0.08 * (4.0 * PI * i as f64 / (n - 1) as f64).cos();
            sinc * w
        })
        .collect();
    let sum: f64 = taps.iter().sum();
    for t in taps.iter_mut() {
        *t /= sum;
    }
    taps.into_iter().map(|t| t as f32).collect()
}

/// Complex FIR with integer decimation. History is kept between calls.
#[derive(Debug, Clone)]
pub struct FirDecim {
    taps: Vec<f32>,
    decim: usize,
    hist: Vec<Complex32>,
    phase: usize,
}

impl FirDecim {
    pub fn new(mut taps: Vec<f32>, decim: usize) -> Self {
        let n = taps.len();
        taps.reverse(); // stored reversed: the inner loop walks both forwards (vectorises)
        FirDecim { taps, decim: decim.max(1), hist: vec![Complex32::new(0.0, 0.0); n - 1], phase: 0 }
    }

    /// Filter and decimate `x`, appending to `out`.
    pub fn process(&mut self, x: &[Complex32], out: &mut Vec<Complex32>) {
        let n = self.taps.len();
        self.hist.extend_from_slice(x);
        // Output sample k uses hist[k .. k + n]
        let mut k = self.phase;
        while k + n <= self.hist.len() {
            out.push(dot_c(&self.hist[k..k + n], &self.taps));
            k += self.decim;
        }
        let consumed = k.min(self.hist.len() - (n - 1));
        self.phase = k - consumed;
        self.hist.drain(..consumed);
    }
}

/// Complex samples times real taps, summed. Eight partial sums so the compiler can keep them
/// in vector registers (a single running sum is a serial dependency chain).
#[inline]
fn dot_c(x: &[Complex32], t: &[f32]) -> Complex32 {
    let mut re = [0.0f32; 8];
    let mut im = [0.0f32; 8];
    let xc = x.chunks_exact(8);
    let tc = t.chunks_exact(8);
    let (xr, tr) = (xc.remainder(), tc.remainder());
    for (a, b) in xc.zip(tc) {
        for j in 0..8 {
            re[j] += a[j].re * b[j];
            im[j] += a[j].im * b[j];
        }
    }
    let mut r: f32 = re.iter().sum();
    let mut i: f32 = im.iter().sum();
    for (a, b) in xr.iter().zip(tr.iter()) {
        r += a.re * b;
        i += a.im * b;
    }
    Complex32::new(r, i)
}

#[inline]
fn dot_r(x: &[f32], t: &[f32]) -> f32 {
    let mut acc = [0.0f32; 8];
    let xc = x.chunks_exact(8);
    let tc = t.chunks_exact(8);
    let (xr, tr) = (xc.remainder(), tc.remainder());
    for (a, b) in xc.zip(tc) {
        for j in 0..8 {
            acc[j] += a[j] * b[j];
        }
    }
    acc.iter().sum::<f32>() + xr.iter().zip(tr.iter()).map(|(a, b)| a * b).sum::<f32>()
}

/// Real FIR (no decimation).
#[derive(Debug, Clone)]
pub struct Fir {
    taps: Vec<f32>,
    hist: Vec<f32>,
}

impl Fir {
    pub fn new(mut taps: Vec<f32>) -> Self {
        let n = taps.len();
        taps.reverse();
        Fir { taps, hist: vec![0.0; n - 1] }
    }
    pub fn process(&mut self, x: &[f32], out: &mut Vec<f32>) {
        let n = self.taps.len();
        self.hist.extend_from_slice(x);
        for k in 0..x.len() {
            out.push(dot_r(&self.hist[k..k + n], &self.taps));
        }
        self.hist.drain(..x.len());
    }
}

/// Decimation by 2, 4 or 8 (a cascade of /2 stages), like SDR++'s source decimation: a
/// narrower span at a lower rate, so less CPU and finer FFT bins for the same FFT size. The
/// central 84% of the output band is kept flat and alias-free.
#[derive(Debug, Clone)]
pub struct Decimator {
    factor: usize,
    stages: Vec<FirDecim>,
    tmp: Vec<Complex32>,
}

impl Decimator {
    /// `factor` 1, 2, 4 or 8 (other values are rounded down to one of these).
    pub fn new(factor: usize, fs_in: f64) -> Self {
        let factor = match factor {
            0 | 1 => 1,
            2 | 3 => 2,
            4..=7 => 4,
            _ => 8,
        };
        let fs_out = fs_in / factor as f64;
        let pass = 0.42 * fs_out;
        let mut stages = Vec::new();
        let mut fs = fs_in;
        while fs > fs_out * 1.5 {
            // After this stage the rate is fs/2: everything that would fold onto [-pass, pass]
            // (at and beyond fs/2 - pass) must be stopped
            let stop = fs / 2.0 - pass;
            stages.push(FirDecim::new(lowpass((pass + stop) / 2.0, stop - pass, fs), 2));
            fs /= 2.0;
        }
        Decimator { factor, stages, tmp: Vec::new() }
    }

    pub fn factor(&self) -> usize {
        self.factor
    }

    /// Decimate `x` into `out` (cleared first).
    pub fn process(&mut self, x: &[Complex32], out: &mut Vec<Complex32>) {
        out.clear();
        if self.stages.is_empty() {
            out.extend_from_slice(x);
            return;
        }
        let mut cur: Vec<Complex32> = x.to_vec();
        for st in self.stages.iter_mut() {
            self.tmp.clear();
            st.process(&cur, &mut self.tmp);
            std::mem::swap(&mut cur, &mut self.tmp);
        }
        out.extend_from_slice(&cur);
    }
}

/// One-pole DC blocker.
#[derive(Debug, Clone, Default)]
pub struct DcBlock {
    x1: f32,
    y1: f32,
}

impl DcBlock {
    pub fn step(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + 0.995 * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Nco;

    fn tone(hz: f64, fs: f64, n: usize) -> Vec<Complex32> {
        let mut nco = Nco::new(-hz, fs);
        (0..n).map(|_| nco.next()).collect()
    }

    fn power(x: &[Complex32]) -> f32 {
        x.iter().map(|v| v.norm_sqr()).sum::<f32>() / x.len() as f32
    }

    #[test]
    fn decimator_passes_and_rejects() {
        let fs = 240_000.0;
        let taps = lowpass(10_000.0, 5_000.0, fs);
        let mut out = Vec::new();
        let mut f = FirDecim::new(taps.clone(), 5);
        f.process(&tone(3_000.0, fs, 20_000), &mut out);
        assert_eq!(out.len(), 20_000 / 5); // history starts zero-filled: one output per 5 inputs
        let p_in = power(&out[out.len() / 2..]);
        assert!((p_in - 1.0).abs() < 0.02, "passband power {p_in}");
        let mut out2 = Vec::new();
        let mut f2 = FirDecim::new(taps, 5);
        f2.process(&tone(40_000.0, fs, 20_000), &mut out2);
        assert!(power(&out2[out2.len() / 2..]) < 1e-5);
    }

    #[test]
    fn decimator_is_block_size_independent() {
        let fs = 100_000.0;
        let x = tone(1234.0, fs, 5000);
        let taps = lowpass(5000.0, 2000.0, fs);
        let mut a = Vec::new();
        FirDecim::new(taps.clone(), 3).process(&x, &mut a);
        let mut b = Vec::new();
        let mut f = FirDecim::new(taps, 3);
        for c in x.chunks(37) {
            f.process(c, &mut b);
        }
        assert_eq!(a.len(), b.len());
        assert!(a.iter().zip(b.iter()).all(|(p, q)| (p - q).norm() < 1e-5));
    }

    #[test]
    fn decimator_by_8_keeps_the_band_and_rejects_aliases() {
        let fs = 2_400_000.0;
        for (hz, inside) in [(100_000.0, true), (-120_000.0, true), (200_000.0, false), (-450_000.0, false), (1_000_000.0, false)] {
            let mut d = Decimator::new(8, fs);
            let mut out = Vec::new();
            d.process(&tone(hz, fs, 240_000), &mut out);
            assert_eq!(out.len(), 30_000);
            let p = power(&out[out.len() / 2..]);
            if inside {
                assert!((p - 1.0).abs() < 0.05, "{hz} Hz: passband power {p}");
            } else {
                assert!(p < 1e-4, "{hz} Hz: alias power {p}");
            }
        }
        assert_eq!(Decimator::new(1, fs).factor(), 1);
        assert_eq!(Decimator::new(4, fs).factor(), 4);
    }
}
