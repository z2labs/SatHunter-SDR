//! Maidenhead locator (4, 6 or 8 characters) <-> latitude / longitude.

/// Centre of the locator square.
pub fn to_lat_lon(loc: &str) -> Option<(f64, f64)> {
    let l: Vec<u8> = loc.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_uppercase().into_bytes();
    if !(l.len() == 4 || l.len() == 6 || l.len() == 8) {
        return None;
    }
    let field = |c: u8| (b'A'..=b'R').contains(&c);
    let sub = |c: u8| (b'A'..=b'X').contains(&c);
    let dig = |c: u8| c.is_ascii_digit();
    if !(field(l[0]) && field(l[1]) && dig(l[2]) && dig(l[3])) {
        return None;
    }
    let mut lon = (l[0] - b'A') as f64 * 20.0 - 180.0 + (l[2] - b'0') as f64 * 2.0;
    let mut lat = (l[1] - b'A') as f64 * 10.0 - 90.0 + (l[3] - b'0') as f64;
    let (mut w, mut h) = (2.0, 1.0);
    if l.len() >= 6 {
        if !(sub(l[4]) && sub(l[5])) {
            return None;
        }
        w /= 24.0;
        h /= 24.0;
        lon += (l[4] - b'A') as f64 * w;
        lat += (l[5] - b'A') as f64 * h;
    }
    if l.len() == 8 {
        if !(dig(l[6]) && dig(l[7])) {
            return None;
        }
        w /= 10.0;
        h /= 10.0;
        lon += (l[6] - b'0') as f64 * w;
        lat += (l[7] - b'0') as f64 * h;
    }
    Some((lat + h / 2.0, lon + w / 2.0))
}

/// 6-character locator, e.g. "JN97ml".
pub fn from_lat_lon(lat: f64, lon: f64) -> String {
    let lon = (lon + 180.0).clamp(0.0, 359.9999);
    let lat = (lat + 90.0).clamp(0.0, 179.9999);
    let c = |base: u8, v: f64| (base + v as u8) as char;
    [
        c(b'A', lon / 20.0),
        c(b'A', lat / 10.0),
        c(b'0', (lon % 20.0) / 2.0),
        c(b'0', lat % 10.0),
        c(b'a', (lon % 2.0) * 12.0),
        c(b'a', (lat % 1.0) * 24.0),
    ]
    .iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn budapest() {
        assert_eq!(from_lat_lon(47.4979, 19.0402), "JN97ml");
        let (la, lo) = to_lat_lon("JN97ml").unwrap();
        assert!((la - 47.479167).abs() < 1e-5 && (lo - 19.041667).abs() < 1e-5);
        assert_eq!(from_lat_lon(la, lo), "JN97ml");
        assert!(to_lat_lon("JN9").is_none());
        assert!(to_lat_lon("jn97").is_some());
    }
}
