//! SatHunterSDR signal processing: spectrum (FFT), channel extraction, demodulators, audio.
//! Pure Rust, allocation-free in the steady state (buffers are reused).

pub use rustfft::num_complex::Complex32;

pub mod channel;
pub mod demod;
pub mod filter;
pub mod spectrum;

/// Numerically controlled oscillator (complex mixer).
#[derive(Debug, Clone)]
pub struct Nco {
    phase: f64,
    inc: f64,
}

impl Nco {
    /// Mixer that shifts a signal at `hz` down to 0 Hz (sample rate `fs`).
    pub fn new(hz: f64, fs: f64) -> Self {
        let mut n = Nco { phase: 0.0, inc: 0.0 };
        n.set(hz, fs);
        n
    }
    pub fn set(&mut self, hz: f64, fs: f64) {
        self.inc = -2.0 * std::f64::consts::PI * hz / fs;
    }
    #[inline]
    pub fn next(&mut self) -> Complex32 {
        let (s, c) = self.phase.sin_cos();
        self.phase += self.inc;
        if self.phase > std::f64::consts::PI {
            self.phase -= 2.0 * std::f64::consts::PI;
        } else if self.phase < -std::f64::consts::PI {
            self.phase += 2.0 * std::f64::consts::PI;
        }
        Complex32::new(c as f32, s as f32)
    }
    pub fn mix(&mut self, x: &mut [Complex32]) {
        for v in x.iter_mut() {
            *v *= self.next();
        }
    }
}
