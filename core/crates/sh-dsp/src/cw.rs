//! Morse (CW) decoder for demodulated audio: finds the beat tone and follows it as it drifts
//! (Doppler left over after tracking), measures the keying envelope with an adaptive
//! threshold, learns the speed from the marks and turns the timing into text.
//! Also reports what it saw (tone, drift, speed, signal-to-noise), so a recording doubles as a
//! measurement of the receiving chain.

use crate::Complex32;
use rustfft::FftPlanner;

const FRAME_N: usize = 2048; // FFT size (~43 ms at 48 kHz, ~23 Hz bins)
const HOP_S: f64 = 0.005; // envelope resolution

#[derive(Debug, Clone, Default)]
pub struct CwReport {
    pub text: String,
    /// (time s, tone Hz) about every 0.5 s while keyed
    pub tone: Vec<(f64, f64)>,
    /// Tone drift, Hz/s (least squares over the keyed part)
    pub drift_hz_s: f64,
    pub wpm: f64,
    pub dot_ms: f64,
    /// Mark level over the noise in one analysis bin, dB; and the same as C/N0, dB-Hz
    pub snr_db: f64,
    pub cn0_dbhz: f64,
    /// Share of the time the key was down
    pub duty: f64,
    /// Seconds with a usable signal
    pub heard_s: f64,
}

const MORSE: [(&str, char); 54] = [
    (".-", 'A'), ("-...", 'B'), ("-.-.", 'C'), ("-..", 'D'), (".", 'E'), ("..-.", 'F'), ("--.", 'G'), ("....", 'H'),
    ("..", 'I'), (".---", 'J'), ("-.-", 'K'), (".-..", 'L'), ("--", 'M'), ("-.", 'N'), ("---", 'O'), (".--.", 'P'),
    ("--.-", 'Q'), (".-.", 'R'), ("...", 'S'), ("-", 'T'), ("..-", 'U'), ("...-", 'V'), (".--", 'W'), ("-..-", 'X'),
    ("-.--", 'Y'), ("--..", 'Z'), ("-----", '0'), (".----", '1'), ("..---", '2'), ("...--", '3'), ("....-", '4'),
    (".....", '5'), ("-....", '6'), ("--...", '7'), ("---..", '8'), ("----.", '9'), (".-.-.-", '.'), ("--..--", ','),
    ("..--..", '?'), ("-..-.", '/'), ("-...-", '='), (".-.-.", '+'), ("-....-", '-'), ("---...", ':'), (".----.", '\''),
    ("-.--.", '('), ("-.--.-", ')'), (".-..-.", '"'), (".--.-.", '@'), ("-.-.--", '!'), ("...-.-", '*'), ("-.-.-.", ';'),
    ("..--.-", '_'), ("...---...", '#'),
];

fn morse(code: &str) -> Option<char> {
    MORSE.iter().find(|(c, _)| *c == code).map(|(_, ch)| *ch)
}

fn code_of(c: char) -> Option<&'static str> {
    let c = c.to_ascii_uppercase();
    MORSE.iter().find(|(_, ch)| *ch == c).map(|(code, _)| *code)
}

/// Encode text as Morse key states, `dot` seconds per unit (for tests and the demo).
pub fn key_pattern(text: &str) -> Vec<bool> {
    let mut out = Vec::new();
    for (wi, word) in text.split_whitespace().enumerate() {
        if wi > 0 {
            out.extend([false; 7]); // word gap: 7 units
        }
        for (ci, ch) in word.chars().enumerate() {
            if ci > 0 {
                out.extend([false; 3]); // letter gap: 3 units
            }
            let code = code_of(ch);
            let Some(code) = code else { continue };
            for (ei, e) in code.chars().enumerate() {
                if ei > 0 {
                    out.push(false);
                }
                out.extend(std::iter::repeat(true).take(if e == '.' { 1 } else { 3 }));
            }
        }
    }
    out
}

fn percentile(v: &mut [f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

/// Two cluster centres (low, high) of 1-D values.
fn kmeans2(v: &[f64]) -> (f64, f64) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let mut w = v.to_vec();
    let (mut c1, mut c2) = (percentile(&mut w, 0.1), percentile(&mut w, 0.9));
    for _ in 0..30 {
        let (mut s1, mut n1, mut s2, mut n2) = (0.0, 0, 0.0, 0);
        for x in v {
            if (x - c1).abs() <= (x - c2).abs() {
                s1 += x;
                n1 += 1;
            } else {
                s2 += x;
                n2 += 1;
            }
        }
        if n1 > 0 {
            c1 = s1 / n1 as f64;
        }
        if n2 > 0 {
            c2 = s2 / n2 as f64;
        }
    }
    (c1.min(c2), c1.max(c2))
}

/// Decode `audio` (mono, `fs` Hz); the tone is looked for in `band` (Hz).
pub fn decode(audio: &[f32], fs: f64, band: (f64, f64)) -> CwReport {
    let hop = (fs * HOP_S).round() as usize;
    if audio.len() < FRAME_N + hop {
        return CwReport::default();
    }
    let bin_hz = fs / FRAME_N as f64;
    let b0 = (band.0 / bin_hz).floor().max(1.0) as usize;
    let b1 = ((band.1 / bin_hz).ceil() as usize).min(FRAME_N / 2 - 1);
    let nb = b1 - b0 + 1;
    // Short-time spectrum of the band
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FRAME_N);
    let win: Vec<f32> = (0..FRAME_N).map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / FRAME_N as f64).cos()) as f32).collect();
    let mut frames: Vec<Vec<f32>> = Vec::new();
    let mut buf = vec![Complex32::new(0.0, 0.0); FRAME_N];
    let mut i = 0;
    while i + FRAME_N <= audio.len() {
        for k in 0..FRAME_N {
            buf[k] = Complex32::new(audio[i + k] * win[k], 0.0);
        }
        fft.process(&mut buf);
        frames.push((b0..=b1).map(|k| buf[k].norm_sqr()).collect());
        i += hop;
    }
    let nf = frames.len();
    let t_of = |f: usize| (f * hop + FRAME_N / 2) as f64 / fs;

    // Tone: strongest bin of each 0.5 s block, kept where it stands out, smoothed, then
    // interpolated to every frame
    let blk = (0.5 / HOP_S) as usize;
    let mut tones: Vec<(usize, f64, f64)> = Vec::new(); // (frame, bin (fractional), prominence dB)
    for s in (0..nf).step_by(blk) {
        let e = (s + blk).min(nf);
        let mut acc = vec![0.0f64; nb];
        for f in &frames[s..e] {
            for (a, v) in acc.iter_mut().zip(f) {
                *a += *v as f64;
            }
        }
        let (k, pk) = acc.iter().enumerate().fold((0, 0.0), |m, (k, v)| if *v > m.1 { (k, *v) } else { m });
        let mut sorted = acc.clone();
        let med = percentile(&mut sorted, 0.5).max(1e-30);
        let prom = 10.0 * (pk / med).log10();
        // parabolic peak interpolation
        let frac = if k > 0 && k + 1 < nb {
            let (a, b, c) = (acc[k - 1].max(1e-30).ln(), acc[k].max(1e-30).ln(), acc[k + 1].max(1e-30).ln());
            let d = a - 2.0 * b + c;
            if d.abs() > 1e-12 { 0.5 * (a - c) / d } else { 0.0 }
        } else {
            0.0
        };
        tones.push(((s + e) / 2, k as f64 + frac, prom));
    }
    // Keep blocks with a clear tone, reject jumps against the neighbours (median of 3)
    let good: Vec<(usize, f64)> = tones.iter().filter(|t| t.2 > 8.0).map(|t| (t.0, t.1)).collect();
    let mut track: Vec<(usize, f64)> = Vec::new();
    for j in 0..good.len() {
        let lo = j.saturating_sub(1);
        let hi = (j + 1).min(good.len() - 1);
        let mut w: Vec<f64> = good[lo..=hi].iter().map(|g| g.1).collect();
        let m = percentile(&mut w, 0.5);
        if (good[j].1 - m).abs() <= 3.0 {
            track.push(good[j]);
        }
    }
    let mut rep = CwReport::default();
    if track.is_empty() {
        return rep;
    }
    let tone_at = |f: usize| -> f64 {
        match track.iter().position(|t| t.0 >= f) {
            Some(0) => track[0].1,
            None => track[track.len() - 1].1,
            Some(p) => {
                let (a, b) = (track[p - 1], track[p]);
                a.1 + (b.1 - a.1) * (f - a.0) as f64 / (b.0 - a.0).max(1) as f64
            }
        }
    };
    rep.tone = track.iter().map(|t| (t_of(t.0), (b0 as f64 + t.1) * bin_hz)).collect();
    if rep.tone.len() >= 2 {
        let n = rep.tone.len() as f64;
        let (mt, mf) = (rep.tone.iter().map(|t| t.0).sum::<f64>() / n, rep.tone.iter().map(|t| t.1).sum::<f64>() / n);
        let sxy: f64 = rep.tone.iter().map(|t| (t.0 - mt) * (t.1 - mf)).sum();
        let sxx: f64 = rep.tone.iter().map(|t| (t.0 - mt).powi(2)).sum();
        rep.drift_hz_s = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    }

    // Envelope on the tone (±1 bin) and the noise beside it (bins 6..20 away)
    let mut env = vec![0.0f64; nf];
    let mut noise = vec![0.0f64; nf];
    for (f, fr) in frames.iter().enumerate() {
        let c = tone_at(f).round() as i64;
        let get = |k: i64| if k >= 0 && (k as usize) < nb { fr[k as usize] as f64 } else { 0.0 };
        env[f] = (get(c - 1) + get(c) + get(c + 1)).max(1e-30);
        let mut nz: Vec<f64> = (6..20).flat_map(|d| [get(c - d), get(c + d)]).filter(|v| *v > 0.0).collect();
        noise[f] = 3.0 * percentile(&mut nz, 0.5).max(1e-30);
    }
    // Short moving average (20 ms) against noise spikes; the noise reference over 1 s
    let ma = |v: &[f64], half: usize| -> Vec<f64> {
        let mut c = vec![0.0; v.len() + 1];
        for (k, x) in v.iter().enumerate() {
            c[k + 1] = c[k] + x;
        }
        (0..v.len()).map(|k| {
            let (a, b) = (k.saturating_sub(half), (k + half + 1).min(v.len()));
            (c[b] - c[a]) / (b - a) as f64
        }).collect()
    };
    let env = ma(&env, 2);
    let noise = ma(&noise, 100);
    let db: Vec<f64> = env.iter().zip(&noise).map(|(e, n)| 10.0 * (e / n).log10()).collect();
    // Adaptive threshold over a 3 s window: half way between the noise and the marks
    let w = (3.0 / HOP_S) as usize;
    let mut key = vec![false; nf];
    let mut active = vec![false; nf]; // a keyed signal is present around this frame
    let mut state = false;
    for f in 0..nf {
        let lo = f.saturating_sub(w / 2);
        let hi = (f + w / 2).min(nf);
        let mut win: Vec<f64> = db[lo..hi].to_vec();
        let floor = percentile(&mut win, 0.25);
        let top = percentile(&mut win, 0.92);
        if top - floor < 6.0 {
            state = false; // nothing keyed here
            continue;
        }
        active[f] = true;
        let thr = floor + 0.5 * (top - floor);
        let hyst = 0.08 * (top - floor);
        state = if state { db[f] > thr - hyst } else { db[f] > thr + hyst };
        key[f] = state;
    }
    // Remove one-frame glitches
    for f in 1..nf.saturating_sub(1) {
        if key[f - 1] == key[f + 1] && key[f] != key[f - 1] {
            key[f] = key[f - 1];
        }
    }
    rep.duty = key.iter().filter(|k| **k).count() as f64 / nf as f64;
    // Signal to noise at the tone itself: power on the tone with the key down against the
    // same bins with the key up (away from the edges, where the keying clicks are); the
    // neighbouring bins would count the clicks as noise
    let settled = |f: usize, v: bool| f >= 10 && f + 10 < nf && active[f] && (f - 10..=f + 10).all(|g| key[g] == v);
    let mut on_p: Vec<f64> = (0..nf).filter(|f| settled(*f, true)).map(|f| env[f]).collect();
    let mut off_p: Vec<f64> = (0..nf).filter(|f| settled(*f, false)).map(|f| env[f]).collect();
    if on_p.len() > 10 && off_p.len() > 10 {
        let (s, n) = (percentile(&mut on_p, 0.5), percentile(&mut off_p, 0.5));
        // env sums 3 bins of a Hann-windowed FFT: noise bandwidth 3 x 1.5 bins
        let nbw = 4.5 * bin_hz;
        rep.snr_db = 10.0 * ((s - n).max(1e-30) / n).log10();
        rep.cn0_dbhz = rep.snr_db + 10.0 * nbw.log10();
    }

    // Runs
    let mut runs: Vec<(bool, f64)> = Vec::new();
    let mut f = 0;
    while f < nf {
        let s = f;
        while f < nf && key[f] == key[s] {
            f += 1;
        }
        runs.push((key[s], (f - s) as f64 * HOP_S));
    }
    if std::env::var("CW_DEBUG").is_ok() {
        eprintln!("runs: {:?}", runs.iter().take(400).map(|r| format!("{}{:.0}", if r.0 { "+" } else { "-" }, r.1 * 1e3)).collect::<Vec<_>>());
    }
    // Marks too short to be a dot (fading, noise spikes) are dropped and the gaps joined
    let mut marks: Vec<f64> = runs.iter().filter(|r| r.0 && r.1 >= 0.04).map(|r| r.1).collect();
    if marks.len() < 3 {
        return rep;
    }
    let rough = percentile(&mut marks, 0.2);
    let mut clean: Vec<(bool, f64)> = Vec::new();
    for (on, d) in runs {
        let on = on && d >= 0.4 * rough;
        match clean.last_mut() {
            Some(l) if l.0 == on => l.1 += d,
            _ => clean.push((on, d)),
        }
    }
    let runs = clean;
    let marks: Vec<f64> = runs.iter().filter(|r| r.0).map(|r| r.1).collect();
    // Marks: two clusters (dots, dashes). The analysis window lengthens the marks and
    // shortens the gaps by about the same amount, so the gaps get their own clusters and
    // every decision is taken half way between the clusters it separates.
    // (longer than ~5 units: a steady carrier, not a dash; left out of the speed estimate)
    let normal: Vec<f64> = marks.iter().copied().filter(|m| *m < 5.0 * rough).collect();
    let (c1, c2) = kmeans2(&normal);
    // (the smearing shrinks the 3:1 dash / dot ratio, to ~2:1 at 20 WPM: 1.5 still tells them apart)
    let (dot_m, mark_thr) = if c2 > 1.5 * c1 { ((c1 + c2 / 3.0) / 2.0, (c1 + c2) / 2.0) } else { (c1.min(c2), 2.0 * c1.min(c2)) };
    // Element gaps (one unit, shortened by the same amount the dots are lengthened): the
    // unit is the mean of the two, then letter gaps are 3 units, word gaps 7
    let mut short_gaps: Vec<f64> = runs.iter().filter(|r| !r.0 && r.1 < 2.0 * dot_m).map(|r| r.1).collect();
    let dot = if short_gaps.len() >= 3 { (dot_m + percentile(&mut short_gaps, 0.5)) / 2.0 } else { dot_m };
    let (letter_thr, word_thr) = (2.0 * dot, 5.0 * dot);
    rep.dot_ms = dot * 1e3;
    rep.wpm = 1.2 / dot;
    rep.heard_s = runs.iter().map(|r| r.1).sum::<f64>() * if rep.duty > 0.0 { 1.0 } else { 0.0 };

    // Timing to text
    let mut text = String::new();
    let mut sym = String::new();
    let flush = |sym: &mut String, text: &mut String| {
        if !sym.is_empty() {
            text.push(morse(sym).unwrap_or('~'));
            sym.clear();
        }
    };
    for (on, d) in runs {
        if on && d > 6.0 * dot {
            // a steady carrier between messages: a separator, not a character
            flush(&mut sym, &mut text);
            if !text.is_empty() && !text.ends_with(' ') {
                text.push(' ');
            }
        } else if on {
            sym.push(if d < mark_thr { '.' } else { '-' });
        } else if d >= word_thr {
            flush(&mut sym, &mut text);
            if !text.is_empty() && !text.ends_with(' ') {
                text.push(' ');
            }
        } else if d >= letter_thr {
            flush(&mut sym, &mut text);
        }
    }
    flush(&mut sym, &mut text);
    rep.text = text.trim().to_string();
    rep
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keyed tone with drift and Gaussian noise; `snr_db` in 100 Hz.
    fn synth(text: &str, wpm: f64, tone: f64, drift: f64, snr_db: f64, fs: f64) -> Vec<f32> {
        let dot = 1.2 / wpm;
        let mut pat = vec![false; 10];
        pat.extend(key_pattern(text));
        pat.extend([false; 10]);
        let n = (pat.len() as f64 * dot * fs) as usize;
        let noise_sigma = (0.5f64 / 10f64.powf(snr_db / 10.0) * (fs / 2.0) / 100.0).sqrt();
        let mut seed = 99u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            ((seed >> 11) as f64 / (1u64 << 53) as f64 - 0.5) * 12f64.sqrt()
        };
        let mut ph = 0.0f64;
        let mut env = 0.0f64;
        (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                let on = pat[((t / dot) as usize).min(pat.len() - 1)];
                env += ((if on { 1.0 } else { 0.0 }) - env) * 0.01; // soft keying
                ph += 2.0 * std::f64::consts::PI * (tone + drift * t) / fs;
                (env * ph.sin() + noise_sigma * rnd()) as f32
            })
            .collect()
    }

    #[test]
    fn decodes_a_drifting_beacon_in_noise() {
        let fs = 48_000.0;
        let x = synth("CQ CQ DE ITUPSAT1 ITUPSAT1 K", 20.0, 2300.0, -4.0, 8.0, fs);
        let r = decode(&x, fs, (300.0, 3500.0));
        assert_eq!(r.text, "CQ CQ DE ITUPSAT1 ITUPSAT1 K", "{r:?}");
        assert!((r.wpm - 20.0).abs() < 2.0, "{} wpm", r.wpm);
        assert!((r.drift_hz_s + 4.0).abs() < 1.0, "drift {}", r.drift_hz_s);
        assert!((r.tone[0].1 - 2300.0).abs() < 20.0, "{:?}", r.tone[0]);
    }

    #[test]
    fn other_speeds() {
        let fs = 48_000.0;
        for wpm in [12.0, 25.0] {
            let x = synth("VVV DE HG0DEMO 73", wpm, 800.0, 0.0, 15.0, fs);
            let r = decode(&x, fs, (300.0, 3500.0));
            assert_eq!(r.text, "VVV DE HG0DEMO 73", "{wpm} wpm: {r:?}");
        }
    }

    #[test]
    fn silence_and_noise_give_nothing() {
        let fs = 48_000.0;
        let r = decode(&vec![0.0; 96_000], fs, (300.0, 3500.0));
        assert!(r.text.is_empty());
        let x = synth("E", 20.0, 1000.0, 0.0, -40.0, fs);
        assert!(decode(&x, fs, (300.0, 3500.0)).text.len() <= 1);
    }
}
