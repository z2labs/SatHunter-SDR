//! Spectrum analyser: windowed FFT, frame rate control, averaging, dBFS output (DC in the middle).
//! Every sample is used: all the FFTs between two frames are averaged in power (Welch), so a
//! weak narrow carrier (a CubeSat CW beacon) stands out of the noise instead of being lost
//! between single snapshots.

use crate::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub struct Spectrum {
    n: usize,
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    buf: Vec<Complex32>,
    scratch: Vec<Complex32>,
    fill: usize,
    /// FFTs per output frame (power averaged)
    per_frame: usize,
    done: usize,
    acc: Vec<f32>,
    avg: Vec<f32>,
    alpha: f32,
    norm_db: f32,
    out: Vec<f32>,
}

impl Spectrum {
    /// `n` bins (power of two recommended), `fs` sample rate, `fps` frames per second,
    /// `avg` averaging 0 (none) .. 0.95 (heavy).
    pub fn new(n: usize, fs: f64, fps: f64, avg: f32) -> Self {
        let n = n.max(16);
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(n);
        // Blackman-Harris (4 term): low leakage, -92 dB sidelobes
        let window: Vec<f32> = (0..n)
            .map(|i| {
                let x = 2.0 * std::f64::consts::PI * i as f64 / n as f64;
                (0.35875 - 0.48829 * x.cos() + 0.14128 * (2.0 * x).cos() - 0.01168 * (3.0 * x).cos()) as f32
            })
            .collect();
        let gain: f32 = window.iter().sum();
        let scratch = vec![Complex32::new(0.0, 0.0); fft.get_inplace_scratch_len()];
        let per_frame = ((fs / fps.max(1.0)) as usize / n).max(1);
        Spectrum {
            n,
            fft,
            window,
            buf: vec![Complex32::new(0.0, 0.0); n],
            scratch,
            fill: 0,
            per_frame,
            done: 0,
            acc: vec![0.0; n],
            avg: vec![-200.0; n],
            alpha: avg.clamp(0.0, 0.99),
            norm_db: 20.0 * gain.log10(),
            out: vec![0.0; n],
        }
    }

    pub fn bins(&self) -> usize {
        self.n
    }

    /// Feed samples; `on_frame` gets each finished frame (dBFS, bin 0 = -fs/2).
    pub fn push(&mut self, mut x: &[Complex32], mut on_frame: impl FnMut(&[f32])) {
        while !x.is_empty() {
            let k = (self.n - self.fill).min(x.len());
            self.buf[self.fill..self.fill + k].copy_from_slice(&x[..k]);
            self.fill += k;
            x = &x[k..];
            if self.fill == self.n {
                self.accumulate();
                self.fill = 0;
                self.done += 1;
                if self.done >= self.per_frame {
                    self.frame();
                    on_frame(&self.out);
                    self.done = 0;
                }
            }
        }
    }

    fn accumulate(&mut self) {
        for (v, w) in self.buf.iter_mut().zip(self.window.iter()) {
            *v *= *w;
        }
        self.fft.process_with_scratch(&mut self.buf, &mut self.scratch);
        for (a, v) in self.acc.iter_mut().zip(self.buf.iter()) {
            *a += v.norm_sqr();
        }
    }

    fn frame(&mut self) {
        let h = self.n / 2;
        let k = 1.0 / self.done.max(1) as f32;
        for i in 0..self.n {
            let p = self.acc[(i + h) % self.n] * k;
            let db = 10.0 * (p + 1e-20).log10() - self.norm_db;
            let a = &mut self.avg[i];
            *a = if *a < -190.0 { db } else { self.alpha * *a + (1.0 - self.alpha) * db };
            self.out[i] = *a;
        }
        self.acc.iter_mut().for_each(|a| *a = 0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Nco;

    #[test]
    fn full_scale_tone_is_0_dbfs_in_the_right_bin() {
        let fs = 1_024_000.0;
        let n = 1024;
        let mut sp = Spectrum::new(n, fs, 1000.0, 0.0);
        let mut nco = Nco::new(-250_000.0, fs); // +250 kHz
        let x: Vec<Complex32> = (0..n * 4).map(|_| nco.next()).collect();
        let mut last = Vec::new();
        sp.push(&x, |f| last = f.to_vec());
        let (imax, vmax) = last.iter().enumerate().fold((0, f32::MIN), |m, (i, v)| if *v > m.1 { (i, *v) } else { m });
        assert_eq!(imax, n / 2 + 250);
        assert!(vmax.abs() < 0.5, "peak {vmax} dBFS");
        let floor = last[100];
        assert!(floor < -80.0, "leakage {floor}");
    }

    /// A carrier about as strong as the noise in its bin: a single FFT's noise peaks (+8 dB
    /// above the median) hide it, the average of a frame's ~40 FFTs makes it the top bin.
    #[test]
    fn averaging_lifts_a_weak_carrier_out_of_the_noise() {
        let fs = 2_400_000.0;
        let n = 2048;
        let mut sp = Spectrum::new(n, fs, 30.0, 0.0);
        let mut nco = Nco::new(-300_000.0, fs);
        let mut seed = 1u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f32 / (1u64 << 53) as f32 - 0.5
        };
        let x: Vec<Complex32> = (0..(fs as usize / 30) * 2).map(|_| nco.next() * 0.004 + Complex32::new(rnd(), rnd()) * 0.2).collect();
        let mut last = Vec::new();
        sp.push(&x, |f| last = f.to_vec());
        let bin = n / 2 + (300_000.0 / fs * n as f64).round() as usize;
        let mut floor: Vec<f32> = last.clone();
        floor.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let median = floor[n / 2];
        let peak = last[bin - 1..=bin + 1].iter().fold(f32::MIN, |m, v| m.max(*v));
        let noise_max = last.iter().enumerate().filter(|(i, _)| i.abs_diff(bin) > 3 && i.abs_diff(n / 2) > 3).fold(f32::MIN, |m, (_, v)| m.max(*v));
        assert!(peak > noise_max + 1.0, "carrier {peak} noise max {noise_max} median {median}");
        let spread = floor[n * 99 / 100] - median;
        assert!(spread < 3.0, "noise spread {spread} dB");
    }
}
