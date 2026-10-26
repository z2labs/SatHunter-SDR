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
    pub fn new(taps: Vec<f32>, decim: usize) -> Self {
        let n = taps.len();
        FirDecim { taps, decim: decim.max(1), hist: vec![Complex32::new(0.0, 0.0); n - 1], phase: 0 }
    }

    /// Filter and decimate `x`, appending to `out`.
    pub fn process(&mut self, x: &[Complex32], out: &mut Vec<Complex32>) {
        let n = self.taps.len();
        self.hist.extend_from_slice(x);
        // Output sample k uses hist[k .. k + n]
        let mut k = self.phase;
        while k + n <= self.hist.len() {
            let w = &self.hist[k..k + n];
            let (mut re, mut im) = (0.0f32, 0.0f32);
            for (s, t) in w.iter().zip(self.taps.iter().rev()) {
                re += s.re * t;
                im += s.im * t;
            }
            out.push(Complex32::new(re, im));
            k += self.decim;
        }
        let consumed = k.min(self.hist.len() - (n - 1));
        self.phase = k - consumed;
        self.hist.drain(..consumed);
    }
}

/// Real FIR (no decimation).
#[derive(Debug, Clone)]
pub struct Fir {
    taps: Vec<f32>,
    hist: Vec<f32>,
}

impl Fir {
    pub fn new(taps: Vec<f32>) -> Self {
        let n = taps.len();
        Fir { taps, hist: vec![0.0; n - 1] }
    }
    pub fn process(&mut self, x: &[f32], out: &mut Vec<f32>) {
        let n = self.taps.len();
        self.hist.extend_from_slice(x);
        for k in 0..x.len() {
            let w = &self.hist[k..k + n];
            out.push(w.iter().zip(self.taps.iter().rev()).map(|(a, b)| a * b).sum());
        }
        self.hist.drain(..x.len());
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
}
