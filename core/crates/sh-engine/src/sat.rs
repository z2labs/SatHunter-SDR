//! Cubesat Hunter state: satellite data on disk, pass list, the selected satellite's pass,
//! its radar track and the satellites overhead.

use serde::Serialize;
use sh_sat::db::{self, LoadStats, RawData};
use sh_sat::orbit::{Look, Station};
use sh_sat::passes::{next_pass, track, Pass};
use sh_sat::tracker::DopplerTracker;
use sh_sat::{Filter, Satellite};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub norad: u64,
    pub name: String,
    pub hu: bool,
    pub fav: bool,
    pub pass: Pass,
    pub freq: f64,
    pub modes: String,
    pub bands: String,
    /// SatNOGS reception of its best listed transmitter: health score, success % and good count
    pub health: f64,
    pub success: f64,
    pub good: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackData {
    pub norad: u64,
    pub t0: f64,
    pub t1: f64,
    pub step: f64,
    pub tmax: f64,
    /// [az, el] pairs
    pub pts: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Other {
    pub name: String,
    pub az: f64,
    pub el: f64,
}

/// A satellite downlink inside the shown span, where it is heard right now (Doppler included),
/// for the labels on the spectrum / waterfall.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Marker {
    pub norad: u64,
    pub name: String,
    /// Received frequency now, Hz (centre of a transponder)
    pub freq: f64,
    /// Linear transponder passband as received, Hz (0, 0 for a single frequency)
    pub low: f64,
    pub high: f64,
    pub mode: String,
    pub el: f64,
    pub selected: bool,
}

/// Downlinks of the satellites above the horizon that fall in `lo..hi` (Hz), strongest
/// candidates first (higher elevation), at most `max`.
pub fn markers(sats: &[Satellite], st: &Station, now: f64, lo: f64, hi: f64, selected: Option<u64>, max: usize) -> Vec<Marker> {
    // Doppler at 70 cm is at most ~±10 kHz; 2.4 GHz ~±60 kHz: pre-filter with a margin
    let margin = 30e-6 * hi.abs().max(lo.abs());
    let mut out = Vec::new();
    for s in sats {
        let txs: Vec<&sh_sat::Transmitter> = s
            .tx
            .iter()
            .filter(|t| t.active() && t.centre() > 0.0)
            .filter(|t| {
                let (a, b) = if t.is_linear() { (t.down_low, t.down_high) } else { (t.down_low, t.down_low) };
                b >= lo - margin && a <= hi + margin
            })
            .collect();
        if txs.is_empty() {
            continue;
        }
        let Ok(look) = s.orbit.look(st, now) else { continue };
        if look.el < 0.0 {
            continue;
        }
        for t in txs {
            let d = |f: f64| sh_sat::doppler(f, look.range_rate_kms);
            let (low, high) = if t.is_linear() { (d(t.down_low), d(t.down_high)) } else { (0.0, 0.0) };
            let freq = d(t.centre());
            let (a, b) = if t.is_linear() { (low, high) } else { (freq, freq) };
            if b < lo || a > hi {
                continue;
            }
            out.push(Marker {
                norad: s.norad,
                name: s.name.clone(),
                freq,
                low,
                high,
                mode: t.mode.clone(),
                el: look.el,
                selected: selected == Some(s.norad),
            });
        }
    }
    out.sort_by(|a, b| b.selected.cmp(&a.selected).then(b.el.partial_cmp(&a.el).unwrap_or(std::cmp::Ordering::Equal)));
    out.truncate(max);
    out
}

pub struct SatState {
    pub sats: Arc<Vec<Satellite>>,
    pub stats: LoadStats,
    pub status: String,
    pub busy: bool,
    pub rows: Arc<Vec<Row>>,
    pub calc_done: usize,
    pub calc_total: usize,
    pub tracking: bool,
    pub tracker: DopplerTracker,
    pub sel_pass: Option<Pass>,
    pub track: Option<TrackData>,
    pub others: Vec<Other>,
    /// Downlinks in the shown span (refreshed once a second)
    pub markers: Vec<Marker>,
    pub last_calc: f64,
}

impl Default for SatState {
    fn default() -> Self {
        SatState {
            sats: Arc::new(Vec::new()),
            stats: LoadStats::default(),
            status: String::new(),
            busy: false,
            rows: Arc::new(Vec::new()),
            calc_done: 0,
            calc_total: 0,
            tracking: false,
            tracker: DopplerTracker::new(10.0),
            sel_pass: None,
            track: None,
            others: Vec::new(),
            markers: Vec::new(),
            last_calc: 0.0,
        }
    }
}

pub fn data_dir(root: &Path) -> PathBuf {
    root.join("sat")
}

/// Age of the cached transmitter list in hours (None = no cache).
pub fn cache_age_hours(root: &Path) -> Option<f64> {
    let m = std::fs::metadata(data_dir(root).join("transmitters.json")).ok()?.modified().ok()?;
    Some(SystemTime::now().duration_since(m).unwrap_or(Duration::ZERO).as_secs_f64() / 3600.0)
}

pub fn read_cache(root: &Path) -> RawData {
    let d = data_dir(root);
    let rd = |f: &str| std::fs::read_to_string(d.join(f)).ok();
    RawData {
        transmitters_json: rd("transmitters.json"),
        tle_satnogs_json: rd("tle_satnogs.json"),
        tle_celestrak_txt: rd("tle_celestrak.txt"),
        satellites_json: rd("satellites.json"),
        network_stats_json: rd("network_stats.json"),
    }
}

/// Download every source into the cache. A failed file keeps its previous copy.
/// Returns "" on success or the first error.
pub fn download(root: &Path, mut progress: impl FnMut(&str)) -> String {
    let d = data_dir(root);
    let _ = std::fs::create_dir_all(&d);
    let mut first_err = String::new();
    let mut tle_ok = 0;
    for (file, url) in db::sources() {
        progress(file);
        let r = (|| -> Result<usize, String> {
            let mut resp = ureq::get(url).header("User-Agent", "SatHunterSDR").call().map_err(|e| e.to_string())?;
            let body = resp.body_mut().with_config().limit(64 << 20).read_to_string().map_err(|e| e.to_string())?;
            if body.len() < 16 {
                return Err("empty reply".into());
            }
            let tmp = d.join(format!("{file}.part"));
            std::fs::write(&tmp, &body).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, d.join(file)).map_err(|e| e.to_string())?;
            Ok(body.len())
        })();
        match r {
            Ok(n) => {
                log::info!("sat data: {file} {n} bytes");
                if file.starts_with("tle") {
                    tle_ok += 1;
                }
            }
            Err(e) => {
                log::warn!("sat data: {file}: {e}");
                if first_err.is_empty() {
                    first_err = format!("{file}: {e}");
                }
                if file == "transmitters.json" {
                    return first_err;
                }
            }
        }
    }
    if tle_ok == 0 && first_err.is_empty() {
        first_err = "no orbit data".into();
    }
    if tle_ok > 0 { String::new() } else { first_err }
}

/// Group for sorting: favourites, then the rest.
fn group(r: &Row) -> u8 {
    if r.fav { 0 } else { 1 }
}

pub fn sort_rows(rows: &mut [Row], now: f64, by: db::SortBy) {
    if by == db::SortBy::Health {
        // Favourites first, then the satellites the ground stations hear best
        rows.sort_by(|a, b| (!a.fav).cmp(&!b.fav).then_with(|| b.health.total_cmp(&a.health)).then_with(|| a.pass.aos.total_cmp(&b.pass.aos)));
        return;
    }
    rows.sort_by(|a, b| {
        group(a)
            .cmp(&group(b))
            .then_with(|| (b.pass.aos <= now).cmp(&(a.pass.aos <= now)))
            .then_with(|| a.pass.aos.total_cmp(&b.pass.aos))
    });
}

/// Candidate rows for the filter: a matching downlink, or a favourite. Passes are not computed yet.
pub fn candidates(sats: &[Satellite], f: &Filter, favs: &BTreeSet<u64>) -> Vec<(usize, Row)> {
    let mut out = Vec::new();
    for (i, s) in sats.iter().enumerate() {
        let fav = favs.contains(&s.norad);
        if f.favourites_only && !fav {
            continue;
        }
        let mut r = Row { norad: s.norad, name: s.name.clone(), hu: s.hungarian, fav, pass: Pass::default(), freq: 0.0, modes: String::new(), bands: String::new(), health: 0.0, success: 0.0, good: 0 };
        let mut any = false;
        let mut bands: Vec<&str> = Vec::new();
        for t in &s.tx {
            if !f.tx_matches(t) && !fav {
                continue;
            }
            any = true;
            if t.health() > r.health {
                r.health = t.health();
                r.success = (t.success() * 100.0).round();
                r.good = t.good;
            }
            if r.freq == 0.0 {
                r.freq = t.centre();
            }
            if !bands.contains(&t.band().name()) {
                bands.push(t.band().name());
            }
            if !t.mode.is_empty() && !r.modes.split(", ").any(|m| m == t.mode) {
                if !r.modes.is_empty() {
                    r.modes.push_str(", ");
                }
                r.modes.push_str(&t.mode);
            }
        }
        r.bands = bands.join(", ");
        if any || fav {
            out.push((i, r));
        }
    }
    // Favourites and Hungarian satellites first, so they are ready first
    out.sort_by_key(|(_, r)| group(r));
    out
}

pub fn compute_pass(s: &Satellite, st: &Station, now: f64, min_el: f64) -> Option<Pass> {
    s.orbit.look(st, now).ok()?;
    next_pass(&s.orbit, st, now, 24.0, min_el)
}

pub fn make_track(s: &Satellite, st: &Station, p: &Pass) -> TrackData {
    let t1 = if p.always { p.aos + 3600.0 } else { p.los };
    let step = 10.0;
    let pts = track(&s.orbit, st, p.aos, t1, step).iter().map(|l: &Look| [l.az as f32, l.el as f32]).collect();
    TrackData { norad: s.norad, t0: p.aos, t1, step, tmax: p.tmax, pts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favourites_then_visible_then_soonest() {
        let mk = |n: u64, fav: bool, hu: bool, aos: f64| Row {
            norad: n, name: n.to_string(), hu, fav, pass: Pass { aos, ..Default::default() }, freq: 0.0, modes: String::new(), bands: String::new(),
            health: n as f64, success: 0.0, good: 0,
        };
        let mut rows = vec![mk(1, false, false, 50.0), mk(2, false, false, 5.0), mk(3, false, true, 90.0), mk(4, true, false, 80.0), mk(5, false, false, 30.0)];
        sort_rows(&mut rows, 10.0, db::SortBy::Pass);
        let order: Vec<u64> = rows.iter().map(|r| r.norad).collect();
        assert_eq!(order, vec![4, 2, 5, 1, 3]);
        // Health: favourite first, then the healthiest
        sort_rows(&mut rows, 10.0, db::SortBy::Health);
        let order: Vec<u64> = rows.iter().map(|r| r.norad).collect();
        assert_eq!(order, vec![4, 5, 3, 2, 1]);
    }

    fn iss() -> Vec<Satellite> {
        let raw = RawData {
            transmitters_json: Some(r#"[
              {"uuid":"aprs","description":"APRS","alive":true,"downlink_low":145825000,"mode":"AFSK","baud":1200,"norad_cat_id":25544,"status":"active","service":"Unknown"},
              {"uuid":"lin","description":"Lin","alive":true,"downlink_low":435760000,"downlink_high":435800000,"mode":"USB","norad_cat_id":25544,"status":"active","service":"Amateur"},
              {"uuid":"dead","description":"Dead","alive":false,"downlink_low":435700000,"mode":"CW","norad_cat_id":25544,"status":"inactive","service":"Amateur"}]"#.into()),
            tle_satnogs_json: Some(r#"[{"tle0":"0 ISS","tle1":"1 25544U 98067A   26282.95044904  .00006711  00000-0  13074-3 0  9998","tle2":"2 25544  51.6315  92.1575 0006672 244.0714 115.9587 15.48795780589546","norad_cat_id":25544}]"#.into()),
            tle_celestrak_txt: None,
            satellites_json: Some(r#"[{"name":"ISS","norad_cat_id":25544,"status":"in orbit","countries":"RU,US"}]"#.into()),
            network_stats_json: None,
        };
        db::load(&raw).unwrap().0
    }

    #[test]
    fn markers_follow_doppler_and_the_span() {
        let sats = iss();
        let st = Station { lat: 47.5, lon: 19.04, alt_m: 120.0 };
        let epoch = 1_791_000_000.0; // around the TLE epoch (Oct 2026)
        let find = |cond: &dyn Fn(f64) -> bool| (0..20_000).map(|k| epoch + k as f64 * 30.0).find(|t| sats[0].orbit.look(&st, *t).map(|l| cond(l.el)).unwrap_or(false));
        let now = find(&|el| el > 10.0).expect("no ISS pass found");
        let look = sats[0].orbit.look(&st, now).unwrap();
        // 2 m span: the APRS downlink, Doppler shifted
        let m = markers(&sats, &st, now, 144.8e6, 146.8e6, Some(25544), 40);
        assert_eq!(m.len(), 1, "{m:?}");
        assert!((m[0].freq - sh_sat::doppler(145.825e6, look.range_rate_kms)).abs() < 1.0);
        assert!(m[0].selected && m[0].mode == "AFSK");
        // 70 cm span: the transponder with its passband, not the dead CW beacon
        let m = markers(&sats, &st, now, 435.0e6, 436.0e6, None, 40);
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(m[0].low < m[0].high && m[0].low > 435.7e6 && m[0].high < 435.9e6, "{m:?}");
        // Another band: nothing; below the horizon: nothing
        assert!(markers(&sats, &st, now, 100e6, 101e6, None, 40).is_empty());
        let down = find(&|el| el < -20.0).unwrap();
        assert!(markers(&sats, &st, down, 144.8e6, 146.8e6, None, 40).is_empty());
    }
}
