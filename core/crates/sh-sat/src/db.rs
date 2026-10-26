//! Satellite database: SatNOGS DB transmitters, satellites and TLEs, CelesTrak TLEs.
//! Parsing only; downloading is the caller's job (see `sources()`).

use crate::orbit::Orbit;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};

/// Where the data comes from: (cache file name, URL).
pub fn sources() -> [(&'static str, &'static str); 5] {
    [
        ("transmitters.json", "https://db.satnogs.org/api/transmitters/?format=json"),
        ("tle_satnogs.json", "https://db.satnogs.org/api/tle/?format=json"),
        ("tle_celestrak.txt", "https://celestrak.org/NORAD/elements/gp.php?GROUP=amateur&FORMAT=tle"),
        ("satellites.json", "https://db.satnogs.org/api/satellites/?format=json"),
        // Observation statistics per transmitter (SatNOGS Network): how often it is received
        ("network_stats.json", "https://network.satnogs.org/api/transmitters/?format=json"),
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Band {
    Hf,
    TwoM,
    SeventyCm,
    TwentyThreeCm,
    SBand,
    ThirteenCm,
    Other,
}

impl Band {
    pub const ALL: [Band; 7] = [Band::TwoM, Band::SeventyCm, Band::TwentyThreeCm, Band::SBand, Band::ThirteenCm, Band::Hf, Band::Other];

    pub fn of(hz: f64) -> Band {
        let m = hz / 1e6;
        match m {
            m if m < 30.0 => Band::Hf,
            m if (144.0..=146.0).contains(&m) => Band::TwoM,
            m if (430.0..=440.0).contains(&m) => Band::SeventyCm,
            m if (1240.0..=1300.0).contains(&m) => Band::TwentyThreeCm,
            m if (2200.0..2300.0).contains(&m) => Band::SBand,
            m if (2300.0..=2450.0).contains(&m) => Band::ThirteenCm,
            _ => Band::Other,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Band::Hf => "HF",
            Band::TwoM => "2 m",
            Band::SeventyCm => "70 cm",
            Band::TwentyThreeCm => "23 cm",
            Band::SBand => "S-band 2.2G",
            Band::ThirteenCm => "13 cm 2.4G",
            Band::Other => "Other",
        }
    }
}

pub fn in_amateur_band(hz: f64) -> bool {
    const B: [(f64, f64); 11] = [
        (21.0, 21.45), (28.0, 29.7), (144.0, 146.0), (430.0, 440.0), (1240.0, 1300.0), (2400.0, 2450.0),
        (3400.0, 3410.0), (5650.0, 5670.0), (5830.0, 5850.0), (10450.0, 10500.0), (24000.0, 24050.0),
    ];
    let m = hz / 1e6;
    B.iter().any(|(a, b)| m >= *a && m <= *b)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Transmitter {
    pub uuid: String,
    pub description: String,
    pub kind: String,    // Transmitter / Transceiver / Transponder
    pub mode: String,    // FM, CW, AFSK, GMSK, BPSK, USB, ...
    pub service: String, // Amateur, Unknown, Earth Exploration, ...
    pub status: String,  // active / inactive / invalid
    pub down_low: f64,   // Hz
    pub down_high: f64,  // Hz, 0 = single frequency
    pub baud: f64,
    pub alive: bool,
    pub invert: bool,
    pub norad: u64,
    /// SatNOGS Network observations of this transmitter: good ones and all vetted ones
    #[serde(default)]
    pub good: u32,
    #[serde(default)]
    pub total: u32,
}

impl Transmitter {
    pub fn is_linear(&self) -> bool {
        self.down_high > self.down_low
    }
    pub fn centre(&self) -> f64 {
        if self.is_linear() { 0.5 * (self.down_low + self.down_high) } else { self.down_low }
    }
    pub fn band(&self) -> Band {
        Band::of(self.centre())
    }
    /// SatNOGS marks many amateur payloads (the ISS included) as "Unknown": go by the band then.
    pub fn amateur(&self) -> bool {
        match self.service.as_str() {
            "Amateur" => true,
            "" | "Unknown" => in_amateur_band(self.centre()),
            _ => false,
        }
    }
    pub fn active(&self) -> bool {
        self.alive && self.status == "active"
    }
    /// Share of good observations, 0..1 (0 when never observed)
    pub fn success(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.good as f64 / self.total as f64 }
    }
    /// How reliably the ground stations hear it: success rate weighted by the number of good
    /// observations (a few lucky passes do not beat thousands of good ones)
    pub fn health(&self) -> f64 {
        self.success() * (1.0 + self.good as f64).ln()
    }
}

#[derive(Debug)]
pub struct Satellite {
    pub norad: u64,
    pub name: String,
    pub line1: String,
    pub line2: String,
    pub orbit: Orbit,
    pub tx: Vec<Transmitter>,
    /// SatNOGS "countries" contains HU
    pub hungarian: bool,
    /// SatNOGS satellite status "in orbit" (older data: "alive"); false for re-entered / future / dead
    pub alive: bool,
}

/// Order of the satellite list
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// Next pass first
    #[default]
    Pass,
    /// Best received on SatNOGS first
    Health,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Filter {
    pub bands: BTreeSet<Band>,
    pub amateur_only: bool,
    pub active_only: bool,
    pub favourites_only: bool,
    pub min_el: f64,
    pub search: String,
    pub sort: SortBy,
}

impl Default for Filter {
    fn default() -> Self {
        Filter {
            bands: [Band::TwoM, Band::SeventyCm, Band::SBand, Band::ThirteenCm].into_iter().collect(),
            amateur_only: true,
            active_only: true,
            favourites_only: false,
            min_el: 5.0,
            search: String::new(),
            sort: SortBy::Pass,
        }
    }
}

impl Filter {
    pub fn tx_matches(&self, t: &Transmitter) -> bool {
        self.bands.contains(&t.band()) && (!self.amateur_only || t.amateur()) && (!self.active_only || t.active())
    }

    pub fn search_matches(&self, s: &Satellite) -> bool {
        let q = self.search.trim().to_lowercase();
        if q.is_empty() {
            return true;
        }
        s.name.to_lowercase().contains(&q)
            || s.norad.to_string() == q
            || s.tx.iter().any(|t| t.mode.to_lowercase().contains(&q) || t.description.to_lowercase().contains(&q))
    }
}

/// SatNOGS satellite status: "in orbit" (older data "alive"), "re-entered", "future", "dead".
pub fn status_alive(s: &str) -> bool {
    !matches!(s.to_lowercase().as_str(), "re-entered" | "dead" | "decayed" | "future")
}

/// Raw downloaded files, any of them may be missing.
#[derive(Debug, Default, Clone)]
pub struct RawData {
    pub transmitters_json: Option<String>,
    pub tle_satnogs_json: Option<String>,
    pub tle_celestrak_txt: Option<String>,
    pub satellites_json: Option<String>,
    pub network_stats_json: Option<String>,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct LoadStats {
    pub satellites: usize,
    pub tles: usize,
    pub transmitters: usize,
    pub names: usize,
    pub hungarian: usize,
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("no transmitter list, update the data")]
    NoTransmitters,
    #[error("no orbit data, update the data")]
    NoOrbits,
    #[error("transmitter list: {0}")]
    Json(String),
}

/// SatNOGS returns a plain array; accept {"results": [...]} (paginated API) as well.
fn list(v: &Value) -> &[Value] {
    match v {
        Value::Array(a) => a,
        Value::Object(o) => o.get("results").and_then(|r| r.as_array()).map(|a| a.as_slice()).unwrap_or(&[]),
        _ => &[],
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
fn f(v: &Value, k: &str) -> f64 {
    v.get(k).and_then(|x| x.as_f64()).unwrap_or(0.0)
}

struct TleSet {
    name: String,
    l1: String,
    l2: String,
    orbit: Orbit,
}

fn add_tle(m: &mut HashMap<u64, TleSet>, name: &str, l1: &str, l2: &str) {
    let Ok(orbit) = Orbit::from_tle(l1, l2) else { return };
    if let Some(old) = m.get(&orbit.norad) {
        if old.orbit.epoch_unix >= orbit.epoch_unix {
            return; // keep the newest element set
        }
    }
    m.insert(orbit.norad, TleSet { name: name.trim().to_string(), l1: l1.trim().to_string(), l2: l2.trim().to_string(), orbit });
}

/// Build the satellite list: every satellite with a usable TLE and at least one downlink.
pub fn load(raw: &RawData) -> Result<(Vec<Satellite>, LoadStats), LoadError> {
    let mut tles: HashMap<u64, TleSet> = HashMap::new();
    if let Some(txt) = &raw.tle_satnogs_json {
        if let Ok(v) = serde_json::from_str::<Value>(txt) {
            for o in list(&v) {
                let n = s(o, "tle0");
                let n = n.strip_prefix("0 ").unwrap_or(&n);
                add_tle(&mut tles, n, &s(o, "tle1"), &s(o, "tle2"));
            }
        }
    }
    if let Some(txt) = &raw.tle_celestrak_txt {
        let lines: Vec<&str> = txt.lines().map(|l| l.trim_end()).filter(|l| !l.trim().is_empty()).collect();
        let mut i = 0;
        while i + 1 < lines.len() {
            if lines[i].starts_with("1 ") && lines[i + 1].starts_with("2 ") {
                add_tle(&mut tles, "", lines[i], lines[i + 1]);
                i += 2;
            } else if i + 2 < lines.len() && lines[i + 1].starts_with("1 ") && lines[i + 2].starts_with("2 ") {
                add_tle(&mut tles, lines[i], lines[i + 1], lines[i + 2]);
                i += 3;
            } else {
                i += 1;
            }
        }
    }
    if tles.is_empty() {
        return Err(LoadError::NoOrbits);
    }

    let mut names: HashMap<u64, String> = HashMap::new();
    let mut status: HashMap<u64, String> = HashMap::new();
    let mut hungarian: BTreeSet<u64> = BTreeSet::new();
    if let Some(txt) = &raw.satellites_json {
        if let Ok(v) = serde_json::from_str::<Value>(txt) {
            for o in list(&v) {
                let norad = f(o, "norad_cat_id") as u64;
                if norad == 0 {
                    continue;
                }
                let n = s(o, "name");
                if !n.is_empty() {
                    names.insert(norad, n);
                }
                status.insert(norad, s(o, "status"));
                // "countries" is a comma separated list of ISO codes, e.g. "HU" or "HU,SK"
                if s(o, "countries").split(',').any(|c| c.trim() == "HU") {
                    hungarian.insert(norad);
                }
            }
        }
    }

    // SatNOGS Network: [{"uuid": "...", "stats": {"good_count": n, "bad_count": n, "unknown_count": n, ...}}]
    let mut obs: HashMap<String, (u32, u32)> = HashMap::new();
    if let Some(txt) = &raw.network_stats_json {
        if let Ok(v) = serde_json::from_str::<Value>(txt) {
            for o in list(&v) {
                let Some(st) = o.get("stats") else { continue };
                let good = f(st, "good_count") as u32;
                // vetted = good + bad (unknown and future ones say nothing yet)
                let vetted = good + f(st, "bad_count") as u32;
                obs.insert(s(o, "uuid"), (good, vetted));
            }
        }
    }

    let txt = raw.transmitters_json.as_ref().ok_or(LoadError::NoTransmitters)?;
    let v: Value = serde_json::from_str(txt).map_err(|e| LoadError::Json(e.to_string()))?;
    let mut txs: HashMap<u64, Vec<Transmitter>> = HashMap::new();
    let mut ntx = 0;
    for o in list(&v) {
        let norad = f(o, "norad_cat_id") as u64;
        // "downlink_drifted" is the observed frequency when SatNOGS knows a drift
        let mut low = f(o, "downlink_drifted");
        if low <= 0.0 {
            low = f(o, "downlink_low");
        }
        if norad == 0 || low <= 0.0 {
            continue;
        }
        let mut high = f(o, "downlink_high");
        if high > 0.0 && high < low {
            std::mem::swap(&mut high, &mut low);
        }
        let sat_alive = status.get(&norad).map(|s| status_alive(s)).unwrap_or(true);
        let uuid = s(o, "uuid");
        let (good, total) = obs.get(&uuid).copied().unwrap_or((0, 0));
        txs.entry(norad).or_default().push(Transmitter {
            good,
            total,
            uuid,
            description: s(o, "description"),
            kind: s(o, "type"),
            mode: s(o, "mode"),
            service: s(o, "service"),
            status: s(o, "status"),
            down_low: low,
            down_high: high,
            baud: f(o, "baud"),
            alive: o.get("alive").and_then(|x| x.as_bool()).unwrap_or(false) && sat_alive,
            invert: o.get("invert").and_then(|x| x.as_bool()).unwrap_or(false),
            norad,
        });
        ntx += 1;
    }

    let mut out = Vec::new();
    let ntle = tles.len();
    for (norad, mut list) in txs {
        let Some(t) = tles.remove(&norad) else { continue };
        list.sort_by(|a, b| a.centre().total_cmp(&b.centre()));
        let mut name = names.get(&norad).cloned().unwrap_or(t.name);
        if name.is_empty() {
            name = format!("NORAD {norad}");
        }
        out.push(Satellite {
            norad,
            name,
            line1: t.l1,
            line2: t.l2,
            orbit: t.orbit,
            tx: list,
            hungarian: hungarian.contains(&norad),
            alive: status.get(&norad).map(|s| status_alive(s)).unwrap_or(true),
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    let stats = LoadStats { satellites: out.len(), tles: ntle, transmitters: ntx, names: names.len(), hungarian: hungarian.len() };
    Ok((out, stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawData {
        RawData {
            transmitters_json: Some(r#"[
              {"uuid":"a","description":"Mode V APRS","alive":true,"type":"Transceiver","downlink_low":145825000,"downlink_drifted":145825000,"mode":"AFSK","baud":1200.0,"norad_cat_id":25544,"status":"active","service":"Unknown"},
              {"uuid":"b","description":"Lin","alive":true,"type":"Transponder","downlink_low":435800000,"downlink_high":435760000,"downlink_drifted":null,"mode":"USB","norad_cat_id":25544,"status":"active","service":"Amateur"},
              {"uuid":"c","description":"S-band telemetry","alive":true,"downlink_low":2250000000,"mode":"FM","norad_cat_id":66756,"status":"active","service":"Unknown"}
            ]"#.into()),
            tle_satnogs_json: Some(r#"[{"tle0":"0 ISS (ZARYA)","tle1":"1 25544U 98067A   26282.95044904  .00006711  00000-0  13074-3 0  9998","tle2":"2 25544  51.6315  92.1575 0006672 244.0714 115.9587 15.48795780589546","norad_cat_id":25544}]"#.into()),
            tle_celestrak_txt: None,
            satellites_json: Some(r#"[{"name":"ISS","norad_cat_id":25544,"status":"in orbit","countries":"RU,US"}]"#.into()),
            network_stats_json: Some(r#"[{"uuid":"a","stats":{"total_count":100,"unknown_count":10,"future_count":0,"good_count":80,"bad_count":10,"success_rate":80}}]"#.into()),
        }
    }

    #[test]
    fn parses_satnogs_records() {
        let (sats, st) = load(&raw()).unwrap();
        assert_eq!(st.satellites, 1); // 66756 has no TLE
        let iss = &sats[0];
        assert_eq!(iss.name, "ISS");
        assert!(iss.alive && !iss.hungarian);
        assert_eq!(iss.tx.len(), 2);
        assert_eq!(iss.tx[0].band(), Band::TwoM);
        assert!(iss.tx[0].amateur()); // "Unknown" service in an amateur band
        assert!(iss.tx[1].is_linear());
        assert_eq!(iss.tx[1].down_low, 435_760_000.0);
        let f = Filter::default();
        assert!(f.tx_matches(&iss.tx[0]));
        let a = iss.tx.iter().find(|t| t.uuid == "a").unwrap();
        assert_eq!((a.good, a.total), (80, 90));
        assert!(a.health() > iss.tx.iter().find(|t| t.uuid == "b").unwrap().health());
        // Old settings without "sort" still load
        let old: Filter = serde_json::from_str(r#"{"amateur_only":false}"#).unwrap();
        assert_eq!(old.sort, SortBy::Pass);
    }

    #[test]
    fn in_orbit_status_is_alive() {
        // The bug of the SDR++ module: SatNOGS says "in orbit", not "alive"
        assert!(status_alive("in orbit"));
        assert!(status_alive("alive"));
        assert!(!status_alive("re-entered"));
    }

    #[test]
    fn celestrak_three_line_sets() {
        let mut r = raw();
        r.tle_satnogs_json = None;
        r.tle_celestrak_txt = Some("ISS (ZARYA)\n1 25544U 98067A   26282.95044904  .00006711  00000-0  13074-3 0  9998\n2 25544  51.6315  92.1575 0006672 244.0714 115.9587 15.48795780589546\n".into());
        let (sats, _) = load(&r).unwrap();
        assert_eq!(sats.len(), 1);
    }
}
