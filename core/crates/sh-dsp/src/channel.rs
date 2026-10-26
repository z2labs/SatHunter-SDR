//! Channel extraction: shift the wanted signal to 0 Hz, low-pass and decimate to a rate a
//! little above what the demodulator needs. Two stages keep it cheap on a phone: a short
//! wide-transition filter for the big decimation, then the sharp channel filter.

use crate::filter::{lowpass, FirDecim};
use crate::{Complex32, Nco};

pub struct Channel {
    nco: Nco,
    stages: Vec<FirDecim>,
    fs_in: f64,
    fs_out: f64,
    offset: f64,
    tmp2: Vec<Complex32>,
}

impl Channel {
    /// `offset` = channel frequency relative to the centre (Hz), `bw` = channel width,
    /// `min_rate` = lowest output rate the demodulator wants.
    pub fn new(fs_in: f64, offset: f64, bw: f64, min_rate: f64) -> Self {
        let want = min_rate.max(bw);
        let d = ((fs_in / want).floor() as usize).max(1);
        let mut stages = Vec::new();
        let fs_out;
        if d >= 8 {
            let db = 4;
            let da = d / db;
            let fs_a = fs_in / da as f64;
            // Stage A: anything that would fold into [-bw/2, bw/2] after decimation is rejected
            let pass = (bw / 2.0).min(fs_a * 0.2);
            let stop = fs_a - pass;
            let taps = lowpass((pass + stop) / 2.0, stop - pass, fs_in);
            stages.push(FirDecim::new(taps, da));
            fs_out = fs_a / db as f64;
            let cutoff = (bw / 2.0).min(fs_out * 0.45);
            stages.push(FirDecim::new(lowpass(cutoff, (cutoff * 0.25).max(fs_out * 0.05), fs_a), db));
        } else {
            fs_out = fs_in / d as f64;
            let cutoff = (bw / 2.0).min(fs_out * 0.45);
            stages.push(FirDecim::new(lowpass(cutoff, (cutoff * 0.25).max(fs_out * 0.05), fs_in), d));
        }
        Channel { nco: Nco::new(offset, fs_in), stages, fs_in, fs_out, offset, tmp2: Vec::new() }
    }

    pub fn rate(&self) -> f64 {
        self.fs_out
    }

    pub fn offset(&self) -> f64 {
        self.offset
    }

    /// Retune without a new filter (Doppler steps, dragging).
    pub fn set_offset(&mut self, offset: f64) {
        self.offset = offset;
        self.nco.set(offset, self.fs_in);
    }

    pub fn process(&mut self, x: &[Complex32], tmp: &mut Vec<Complex32>, out: &mut Vec<Complex32>) {
        tmp.clear();
        tmp.extend(x.iter().map(|v| *v * self.nco.next()));
        if self.stages.len() == 1 {
            self.stages[0].process(tmp, out);
        } else {
            self.tmp2.clear();
            self.stages[0].process(tmp, &mut self.tmp2);
            self.stages[1].process(&self.tmp2, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, fs: f64, n: usize) -> Vec<Complex32> {
        let mut nco = Nco::new(-hz, fs);
        (0..n).map(|_| nco.next()).collect()
    }
    fn power(x: &[Complex32]) -> f32 {
        x.iter().map(|v| v.norm_sqr()).sum::<f32>() / x.len() as f32
    }

    #[test]
    fn extracts_the_channel_at_2_4_msps() {
        let fs = 2_400_000.0;
        let mut ch = Channel::new(fs, 300_000.0, 16_000.0, 48_000.0);
        assert!(ch.rate() >= 48_000.0 && ch.rate() < 60_000.0, "rate {}", ch.rate());
        let (mut tmp, mut out) = (Vec::new(), Vec::new());
        ch.process(&tone(303_000.0, fs, 240_000), &mut tmp, &mut out);
        assert!((power(&out[out.len() / 2..]) - 1.0).abs() < 0.03);
        // A strong signal 100 kHz away (folds near the channel after stage A) is gone
        let mut ch2 = Channel::new(fs, 300_000.0, 16_000.0, 48_000.0);
        let mut out2 = Vec::new();
        ch2.process(&tone(400_000.0, fs, 240_000), &mut tmp, &mut out2);
        assert!(power(&out2[out2.len() / 2..]) < 1e-5, "{}", power(&out2[out2.len() / 2..]));
    }
}
