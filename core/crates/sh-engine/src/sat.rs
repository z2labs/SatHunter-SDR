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

/// Group for sorting: favourites, then Hungarian satellites, then the rest.
fn group(r: &Row) -> u8 {
    if r.fav {
        0
    } else if r.hu {
        1
    } else {
        2
    }
}

pub fn sort_rows(rows: &mut [Row], now: f64) {
    rows.sort_by(|a, b| {
        group(a)
            .cmp(&group(b))
            .then_with(|| (b.pass.aos <= now).cmp(&(a.pass.aos <= now)))
            .then_with(|| a.pass.aos.total_cmp(&b.pass.aos))
    });
}

/// Candidate rows for the filter: a matching downlink, or a favourite, or a Hungarian satellite
/// (alive only when "active only" is set). Passes are not computed yet.
pub fn candidates(sats: &[Satellite], f: &Filter, favs: &BTreeSet<u64>) -> Vec<(usize, Row)> {
    let mut out = Vec::new();
    for (i, s) in sats.iter().enumerate() {
        let fav = favs.contains(&s.norad);
        if f.favourites_only && !fav {
            continue;
        }
        let mut r = Row { norad: s.norad, name: s.name.clone(), hu: s.hungarian, fav, pass: Pass::default(), freq: 0.0, modes: String::new(), bands: String::new() };
        let mut any = false;
        let mut bands: Vec<&str> = Vec::new();
        for t in &s.tx {
            let special = fav || (s.hungarian && (!f.active_only || (s.alive && t.active())));
            if !f.tx_matches(t) && !special {
                continue;
            }
            any = true;
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
    fn favourites_then_hungarian_then_visible_first() {
        let mk = |n: u64, fav: bool, hu: bool, aos: f64| Row {
            norad: n, name: n.to_string(), hu, fav, pass: Pass { aos, ..Default::default() }, freq: 0.0, modes: String::new(), bands: String::new(),
        };
        let mut rows = vec![mk(1, false, false, 50.0), mk(2, false, false, 5.0), mk(3, false, true, 90.0), mk(4, true, false, 80.0), mk(5, false, false, 30.0)];
        sort_rows(&mut rows, 10.0);
        let order: Vec<u64> = rows.iter().map(|r| r.norad).collect();
        assert_eq!(order, vec![4, 3, 2, 5, 1]);
    }
}
