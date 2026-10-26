//! Pass prediction: next AOS / max / LOS and the sky track for the radar.

use crate::orbit::{Look, Orbit, Station};

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize)]
pub struct Pass {
    /// Above the horizon for the whole search window (GEO, HEO apogee)
    pub always: bool,
    pub aos: f64, // unix seconds (in the past for an ongoing pass)
    pub los: f64,
    pub tmax: f64,
    pub max_el: f64,
    pub aos_az: f64,
    pub los_az: f64,
}

fn el(o: &Orbit, st: &Station, t: f64) -> f64 {
    o.look(st, t).map(|l| l.el).unwrap_or(-90.0)
}

/// Bisection to 1 s between a and b, where the elevation crosses 0.
fn edge(o: &Orbit, st: &Station, mut a: f64, mut b: f64, rising: bool) -> f64 {
    for _ in 0..20 {
        if b - a <= 1.0 {
            break;
        }
        let m = 0.5 * (a + b);
        if (el(o, st, m) > 0.0) == rising {
            b = m;
        } else {
            a = m;
        }
    }
    0.5 * (a + b)
}

/// Next pass with a maximum elevation of at least `min_el`, searching `hours` from `now`.
/// An ongoing pass is returned with its AOS in the past.
pub fn next_pass(o: &Orbit, st: &Station, now: f64, hours: f64, min_el: f64) -> Option<Pass> {
    let end = now + hours * 3600.0;
    let step_for = |e: f64| (-e * 8.0).clamp(20.0, 300.0);
    let mut t = now;
    let mut e = el(o, st, t);
    while t < end {
        let aos;
        if e > 0.0 {
            // Above the horizon: walk back to the AOS of this pass (at most 3 h)
            let mut a = t;
            while a > t - 3.0 * 3600.0 && el(o, st, a) > 0.0 {
                a -= 60.0;
            }
            aos = if el(o, st, a) > 0.0 { a } else { edge(o, st, a, t, true) };
        } else {
            let (mut t1, mut e1) = (t, e);
            let mut found = None;
            while t1 < end {
                let t2 = t1 + step_for(e1);
                let e2 = el(o, st, t2);
                if e2 > 0.0 {
                    found = Some((edge(o, st, t1, t2, true), ()));
                    break;
                }
                t1 = t2;
                e1 = e2;
            }
            let (a, _) = found?;
            aos = a;
        }
        // Walk the pass: maximum and LOS
        let mut tm = aos.max(now);
        let mut em = el(o, st, tm);
        let mut tt = tm;
        let mut los = None;
        while tt < end + 12.0 * 3600.0 && tt <= aos + 12.0 * 3600.0 {
            let t2 = tt + 15.0;
            let e2 = el(o, st, t2);
            if e2 > em {
                em = e2;
                tm = t2;
            }
            if e2 <= 0.0 {
                los = Some(edge(o, st, tt, t2, false));
                break;
            }
            tt = t2;
        }
        // Refine the maximum (ternary search over +-15 s)
        let (mut lo, mut hi) = (tm - 15.0, tm + 15.0);
        for _ in 0..12 {
            let (m1, m2) = (lo + (hi - lo) / 3.0, hi - (hi - lo) / 3.0);
            if el(o, st, m1) < el(o, st, m2) {
                lo = m1;
            } else {
                hi = m2;
            }
        }
        let tp = 0.5 * (lo + hi);
        let ep = el(o, st, tp);
        if ep > em {
            em = ep;
            tm = tp;
        }
        let az = |t: f64| o.look(st, t).map(|l| l.az).unwrap_or(0.0);
        match los {
            None => {
                let a = az(aos);
                return Some(Pass { always: true, aos, los: end, tmax: tm, max_el: em, aos_az: a, los_az: a });
            }
            Some(los) if em >= min_el => {
                return Some(Pass { always: false, aos, los, tmax: tm, max_el: em, aos_az: az(aos), los_az: az(los) });
            }
            Some(los) => {
                t = los + 60.0;
                e = el(o, st, t);
            }
        }
    }
    None
}

/// Sky track for the radar: (az, el) every `step` seconds from t0 to t1 (last point at t1).
pub fn track(o: &Orbit, st: &Station, t0: f64, t1: f64, step: f64) -> Vec<Look> {
    if t1 <= t0 || step <= 0.0 {
        return Vec::new();
    }
    let n = (((t1 - t0) / step) as usize + 2).min(2000);
    (0..n).filter_map(|i| o.look(st, (t0 + i as f64 * step).min(t1)).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const ISS1: &str = "1 25544U 98067A   26282.95044904  .00006711  00000-0  13074-3 0  9998";
    const ISS2: &str = "2 25544  51.6315  92.1575 0006672 244.0714 115.9587 15.48795780589546";

    #[test]
    fn iss_passes_match_skyfield() {
        // Skyfield find_events for Budapest from 2026-10-14 04:00 UTC: (aos, culmination, max el, los)
        let reference = [
            (1791954721.0, 1791955046.0, 51.88, 1791955374.0),
            (1791960532.0, 1791960860.0, 47.69, 1791961188.0),
            (1791966367.0, 1791966694.0, 42.07, 1791967019.0),
            (1791972184.0, 1791972514.0, 75.18, 1791972843.0),
        ];
        let o = Orbit::from_tle(ISS1, ISS2).unwrap();
        let st = Station { lat: 47.4979, lon: 19.0402, alt_m: 120.0 };
        let mut t = 1791950400.0;
        for (aos, tmax, max_el, los) in reference {
            let p = next_pass(&o, &st, t, 24.0, 0.0).expect("pass");
            assert!((p.aos - aos).abs() < 3.0, "aos {} vs {}", p.aos, aos);
            assert!((p.los - los).abs() < 3.0, "los {} vs {}", p.los, los);
            assert!((p.tmax - tmax).abs() < 5.0, "tmax {} vs {}", p.tmax, tmax);
            assert!((p.max_el - max_el).abs() < 0.05, "max el {} vs {}", p.max_el, max_el);
            t = p.los + 60.0;
        }
    }

    #[test]
    fn min_elevation_skips_low_passes() {
        let o = Orbit::from_tle(ISS1, ISS2).unwrap();
        let st = Station { lat: 47.4979, lon: 19.0402, alt_m: 120.0 };
        // From 2026-10-14 04:00 the first pass with >= 60 deg is the 75 deg one
        let p = next_pass(&o, &st, 1791950400.0, 24.0, 60.0).unwrap();
        assert!((p.max_el - 75.18).abs() < 0.05);
    }
}
