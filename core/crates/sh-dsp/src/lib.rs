//! SatHunterSDR signal processing: spectrum (FFT), channel extraction, demodulators, audio.
//! Pure Rust, allocation-free in the steady state (buffers are reused).

pub use rustfft::num_complex::Complex32;

pub mod channel;
pub mod demod;
pub mod filter;
pub mod spectrum;

/// Numerically controlled oscillator (complex mixer). A rotating phasor (one complex multiply
/// per sample) instead of a sin/cos per sample, renormalised now and then; f64 so the
/// frequency stays exact over long runs.
#[derive(Debug, Clone)]
pub struct Nco {
    re: f64,
    im: f64,
    rot_re: f64,
    rot_im: f64,
    inc: f64,
    n: u32,
}

impl Nco {
    /// Mixer that shifts a signal at `hz` down to 0 Hz (sample rate `fs`).
    pub fn new(hz: f64, fs: f64) -> Self {
        let mut n = Nco { re: 1.0, im: 0.0, rot_re: 1.0, rot_im: 0.0, inc: 0.0, n: 0 };
        n.set(hz, fs);
        n
    }
    pub fn set(&mut self, hz: f64, fs: f64) {
        self.inc = -2.0 * std::f64::consts::PI * hz / fs;
        let (s, c) = self.inc.sin_cos();
        self.rot_re = c;
        self.rot_im = s;
    }
    #[inline]
    pub fn next(&mut self) -> Complex32 {
        let out = Complex32::new(self.re as f32, self.im as f32);
        let re = self.re * self.rot_re - self.im * self.rot_im;
        let im = self.re * self.rot_im + self.im * self.rot_re;
        self.re = re;
        self.im = im;
        self.n += 1;
        if self.n >= 1024 {
            self.n = 0;
            let k = 1.0 / (re * re + im * im).sqrt();
            self.re *= k;
            self.im *= k;
        }
        out
    }
    pub fn mix(&mut self, x: &mut [Complex32]) {
        for v in x.iter_mut() {
            *v *= self.next();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nco_matches_sin_cos_over_ten_seconds() {
        let fs = 2_400_000.0;
        let hz = 123_456.789;
        let mut n = Nco::new(hz, fs);
        let total = 24_000_000usize;
        let mut last = Complex32::new(0.0, 0.0);
        for _ in 0..total {
            last = n.next();
        }
        let ph = -2.0 * std::f64::consts::PI * hz / fs * (total - 1) as f64;
        let want = Complex32::new(ph.cos() as f32, ph.sin() as f32);
        assert!((last - want).norm() < 1e-3, "{last} vs {want}");
        assert!((last.norm() - 1.0).abs() < 1e-5);
    }
}
