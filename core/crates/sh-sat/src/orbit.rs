//! Orbit propagation (SGP4) and topocentric look angles, range rate and Doppler.

use std::f64::consts::PI;

const DEG: f64 = PI / 180.0;
const WGS84_A: f64 = 6378.137; // km
const WGS84_F: f64 = 1.0 / 298.257_223_563;
const EARTH_ROT: f64 = 7.292_115_146_706_979e-5; // rad/s
/// Speed of light, km/s
pub const C_KMS: f64 = 299_792.458;

/// Ground station (observer) position.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Station {
    pub lat: f64,  // degrees north
    pub lon: f64,  // degrees east
    pub alt_m: f64,
}

impl Default for Station {
    fn default() -> Self {
        // Budapest
        Station { lat: 47.4979, lon: 19.0402, alt_m: 120.0 }
    }
}

impl Station {
    fn ecef(&self) -> [f64; 3] {
        let (lat, lon) = (self.lat * DEG, self.lon * DEG);
        let e2 = WGS84_F * (2.0 - WGS84_F);
        let n = WGS84_A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
        let h = self.alt_m / 1000.0;
        [
            (n + h) * lat.cos() * lon.cos(),
            (n + h) * lat.cos() * lon.sin(),
            (n * (1.0 - e2) + h) * lat.sin(),
        ]
    }
}

/// Where a satellite is, seen from the station.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Look {
    pub az: f64,             // degrees, 0 = north, clockwise
    pub el: f64,             // degrees
    pub range_km: f64,
    pub range_rate_kms: f64, // positive = moving away
}

/// Received frequency for a transmitted one (first order is plenty for LEO: |v/c| < 3e-5).
pub fn doppler(f_tx: f64, range_rate_kms: f64) -> f64 {
    f_tx * (1.0 - range_rate_kms / C_KMS)
}

/// Greenwich mean sidereal time (IAU 1982, as used for TEME), radians.
fn gmst(unix: f64) -> f64 {
    let jd = unix / 86400.0 + 2_440_587.5;
    let t = (jd - 2_451_545.0) / 36525.0;
    let sec = 67310.54841 + (876_600.0 * 3600.0 + 8_640_184.812_866) * t + 0.093_104 * t * t - 6.2e-6 * t * t * t;
    (sec.rem_euclid(86400.0) / 240.0) * DEG
}

#[derive(Debug, thiserror::Error)]
pub enum OrbitError {
    #[error("bad TLE: {0}")]
    Tle(String),
    #[error("propagation failed: {0}")]
    Propagate(String),
}

/// A satellite's orbit, ready to propagate.
pub struct Orbit {
    pub norad: u64,
    pub epoch_unix: f64,
    constants: sgp4::Constants,
}

impl std::fmt::Debug for Orbit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Orbit(norad {}, epoch {})", self.norad, self.epoch_unix)
    }
}

impl Orbit {
    pub fn from_tle(line1: &str, line2: &str) -> Result<Orbit, OrbitError> {
        let el = sgp4::Elements::from_tle(None, line1.trim().as_bytes(), line2.trim().as_bytes())
            .map_err(|e| OrbitError::Tle(e.to_string()))?;
        let constants = sgp4::Constants::from_elements(&el).map_err(|e| OrbitError::Tle(e.to_string()))?;
        let epoch_unix = el.datetime.and_utc().timestamp_millis() as f64 / 1000.0;
        Ok(Orbit { norad: el.norad_id, epoch_unix, constants })
    }

    /// Position and velocity in the Earth-fixed frame (km, km/s).
    fn ecef(&self, unix: f64) -> Result<([f64; 3], [f64; 3]), OrbitError> {
        let p = self
            .constants
            .propagate(sgp4::MinutesSinceEpoch((unix - self.epoch_unix) / 60.0))
            .map_err(|e| OrbitError::Propagate(e.to_string()))?;
        let th = gmst(unix);
        let (s, c) = th.sin_cos();
        let r = [c * p.position[0] + s * p.position[1], -s * p.position[0] + c * p.position[1], p.position[2]];
        let v0 = [c * p.velocity[0] + s * p.velocity[1], -s * p.velocity[0] + c * p.velocity[1], p.velocity[2]];
        // Earth rotation: v_ecef = R v_teme - w x r_ecef
        let v = [v0[0] + EARTH_ROT * r[1], v0[1] - EARTH_ROT * r[0], v0[2]];
        Ok((r, v))
    }

    /// Look angles, range and range rate at a unix time.
    pub fn look(&self, st: &Station, unix: f64) -> Result<Look, OrbitError> {
        let (r, v) = self.ecef(unix)?;
        let o = st.ecef();
        let d = [r[0] - o[0], r[1] - o[1], r[2] - o[2]];
        let range = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let (lat, lon) = (st.lat * DEG, st.lon * DEG);
        let (slat, clat, slon, clon) = (lat.sin(), lat.cos(), lon.sin(), lon.cos());
        let e = -slon * d[0] + clon * d[1];
        let n = -slat * clon * d[0] - slat * slon * d[1] + clat * d[2];
        let u = clat * clon * d[0] + clat * slon * d[1] + slat * d[2];
        let az = e.atan2(n).rem_euclid(2.0 * PI) / DEG;
        let el = (u / range).clamp(-1.0, 1.0).asin() / DEG;
        let rr = (d[0] * v[0] + d[1] * v[1] + d[2] * v[2]) / range;
        Ok(Look { az, el, range_km: range, range_rate_kms: rr })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const ISS1: &str = "1 25544U 98067A   26282.95044904  .00006711  00000-0  13074-3 0  9998";
    pub const ISS2: &str = "2 25544  51.6315  92.1575 0006672 244.0714 115.9587 15.48795780589546";

    #[test]
    fn iss_look_matches_skyfield() {
        // Reference: Skyfield 1.x, Budapest 47.4979 N 19.0402 E 120 m, 2026-10-14 11:07:22.8 UTC
        let o = Orbit::from_tle(ISS1, ISS2).unwrap();
        let st = Station { lat: 47.4979, lon: 19.0402, alt_m: 120.0 };
        let l = o.look(&st, 1791950400.0 + 3600.0 * 7.123).unwrap();
        assert!((l.az - 314.49797).abs() < 0.02, "az {}", l.az);
        assert!((l.el - -69.39707).abs() < 0.02, "el {}", l.el);
        assert!((l.range_rate_kms - -2.392072).abs() < 0.002, "rr {}", l.range_rate_kms);
    }
}
