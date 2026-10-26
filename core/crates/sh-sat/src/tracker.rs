//! Doppler tracking: which frequency to tune to, and the user's own retune kept as an offset
//! in the satellite's frame (so a linear transponder or a drifting beacon stays tracked).

use crate::db::Transmitter;
use crate::orbit::{doppler, Look};
use crate::passes::Pass;

/// Demodulator suggested for a downlink.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub enum Demod {
    Nfm,
    Wfm,
    Am,
    Usb,
    Lsb,
    Cw,
}

/// Demodulator and bandwidth (Hz) for a transmitter's mode.
pub fn demod_for(t: &Transmitter) -> (Demod, f64) {
    let m = t.mode.to_uppercase();
    if t.is_linear() || m == "USB" || m == "SSB" {
        return (Demod::Usb, 2800.0);
    }
    match m.as_str() {
        "LSB" => (Demod::Lsb, 2800.0),
        "CW" => (Demod::Cw, 500.0),
        "AM" => (Demod::Am, 10_000.0),
        "FM" | "AFSK" | "FMN" | "APRS" => (Demod::Nfm, 16_000.0),
        // Data modes: NFM wide enough for the symbol rate, so the signal is seen and heard
        _ => {
            let bd = if t.baud > 0.0 { t.baud } else { 9600.0 };
            (Demod::Nfm, (2.0 * bd + 6000.0).clamp(12_500.0, 50_000.0))
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DopplerTracker {
    /// Hz in the satellite's frame, from the user's own retuning
    pub user_offset: f64,
    /// Last frequency we tuned to (0 = none yet)
    last_cmd: f64,
    /// Minimum change before retuning, Hz
    pub step_hz: f64,
}

impl DopplerTracker {
    pub fn new(step_hz: f64) -> Self {
        DopplerTracker { user_offset: 0.0, last_cmd: 0.0, step_hz }
    }

    pub fn reset(&mut self) {
        self.user_offset = 0.0;
        self.last_cmd = 0.0;
    }

    /// Frequency the receiver should hear now. Below the horizon (AOS within an hour) the
    /// receiver waits on the AOS frequency, ready for the signal.
    pub fn target(&self, tx_hz: f64, now: &Look, aos: Option<&Look>) -> f64 {
        let rr = match aos {
            Some(a) if now.el < 0.0 => a.range_rate_kms,
            _ => now.range_rate_kms,
        };
        doppler(tx_hz + self.user_offset, rr)
    }

    /// One tracking step. `current` is where the receiver is tuned now (the user may have
    /// moved it). Returns Some(frequency) when the receiver should be retuned.
    pub fn step(&mut self, tx_hz: f64, now: &Look, aos: Option<&Look>, current: f64) -> Option<f64> {
        if self.last_cmd != 0.0 && (current - self.last_cmd).abs() > 1.0 {
            let target = self.target(tx_hz, now, aos);
            let factor = if tx_hz > 0.0 { target / (tx_hz + self.user_offset) } else { 1.0 };
            self.user_offset += (current - self.last_cmd) / factor;
        }
        let target = self.target(tx_hz, now, aos);
        if self.last_cmd == 0.0 || (target - self.last_cmd).abs() >= self.step_hz {
            self.last_cmd = target;
            return Some(target);
        }
        None
    }
}

/// Should the AOS look be used (below the horizon, pass starting within an hour)?
pub fn waiting_for_aos(now_el: f64, pass: Option<&Pass>, now: f64) -> bool {
    matches!(pass, Some(p) if now_el < 0.0 && !p.always && p.aos > now && p.aos - now < 3600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look(rr: f64, el: f64) -> Look {
        Look { az: 0.0, el, range_km: 1000.0, range_rate_kms: rr }
    }

    #[test]
    fn follows_doppler_in_steps() {
        let mut t = DopplerTracker::new(10.0);
        let f0 = 435_000_000.0;
        let a = t.step(f0, &look(-7.0, 10.0), None, 0.0).unwrap();
        assert!((a - f0 * (1.0 + 7.0 / 299_792.458)).abs() < 0.01);
        // Tiny change: no retune
        assert!(t.step(f0, &look(-6.99999, 10.0), None, a).is_none());
        // 1 km/s later: retune
        assert!(t.step(f0, &look(-6.0, 10.0), None, a).is_some());
    }

    #[test]
    fn manual_retune_kept_as_offset() {
        let mut t = DopplerTracker::new(10.0);
        let f0 = 435_780_000.0;
        let a = t.step(f0, &look(-5.0, 20.0), None, 0.0).unwrap();
        // User moves 2 kHz up on the transponder
        let b = t.step(f0, &look(-5.0, 20.0), None, a + 2000.0);
        assert!(b.is_none() || (b.unwrap() - (a + 2000.0)).abs() < 1.0);
        assert!((t.user_offset - 2000.0).abs() < 1.0);
        // Later in the pass the offset is still applied
        let c = t.step(f0, &look(3.0, 20.0), None, a + 2000.0).unwrap();
        assert!((c - doppler(f0 + t.user_offset, 3.0)).abs() < 0.01);
    }

    #[test]
    fn waits_on_aos_frequency() {
        let t = DopplerTracker::new(10.0);
        let f = t.target(145_800_000.0, &look(0.5, -30.0), Some(&look(-7.0, 0.0)));
        assert!(f > 145_800_000.0 + 3000.0);
    }

    #[test]
    fn demod_from_mode() {
        let mut tx = Transmitter {
            uuid: "x".into(), description: String::new(), kind: String::new(), mode: "FM".into(),
            service: String::new(), status: "active".into(), down_low: 145.8e6, down_high: 0.0,
            baud: 0.0, alive: true, invert: false, norad: 1,
        };
        assert_eq!(demod_for(&tx).0, Demod::Nfm);
        tx.mode = "CW".into();
        assert_eq!(demod_for(&tx), (Demod::Cw, 500.0));
        tx.mode = "GMSK".into();
        tx.baud = 9600.0;
        assert_eq!(demod_for(&tx), (Demod::Nfm, 25_200.0));
        tx.down_high = 435.9e6;
        assert_eq!(demod_for(&tx).0, Demod::Usb);
    }
}
