//! Play an IQ recording (the app's, or an SDR++ baseband WAV) back through the app's DSP and
//! report: what the receiver knew, the strongest carriers, the signal at the VFO, a CW decode,
//! and a waterfall image.
//!
//! `cargo run --release -p sh-engine --example iq_analyze -- rec.wav [vfo_hz] [waterfall.pgm]`
use sh_dsp::demod::{Demodulator, Mode, AUDIO_RATE};
use sh_dsp::spectrum::Spectrum;
use sh_engine::record::read_wav;
use std::io::Write;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let path = a.get(1).expect("usage: iq_analyze rec.wav [vfo_hz] [waterfall.pgm]");
    let (fs, iq, meta) = read_wav(&std::fs::read(path).expect("read")).expect("wav");
    // centre: from the metadata, else from an SDR++ style name (baseband_<Hz>Hz_...)
    let center = meta["center_hz"].as_f64().or_else(|| {
        let n = std::path::Path::new(path).file_name()?.to_str()?;
        n.strip_prefix("baseband_")?.split("Hz").next()?.parse().ok()
    }).unwrap_or(0.0);
    let vfo = a.get(2).and_then(|s| s.parse().ok()).or(meta["vfo_hz"].as_f64()).unwrap_or(center);
    println!("file      : {path}");
    println!("length    : {:.1} s at {:.0} S/s, centre {:.6} MHz", iq.len() as f64 / fs, fs, center / 1e6);
    if !meta.is_null() {
        for k in ["satellite", "transmitter", "mode", "gain_db", "ppm", "decimation", "bias_tee", "tracking", "source_status", "dropped_blocks", "retunes"] {
            if !meta[k].is_null() {
                println!("{k:10}: {}", meta[k]);
            }
        }
    }
    // Average spectrum, strongest narrow carriers
    let n = 8192;
    let mut sp = Spectrum::new(n, fs, 1.0 / (iq.len() as f64 / fs).max(1.0), 0.0);
    let mut avg = vec![0.0f64; n];
    let mut frames = 0;
    let mut rows: Vec<Vec<f32>> = Vec::new();
    let mut sp_rows = Spectrum::new(1024, fs, 10.0, 0.0);
    sp_rows.push(&iq, |f| rows.push(f.to_vec()));
    sp.push(&iq, |f| {
        for (a, v) in avg.iter_mut().zip(f) {
            *a += 10f64.powf(*v as f64 / 10.0);
        }
        frames += 1;
    });
    if frames > 0 {
        let db: Vec<f64> = avg.iter().map(|v| 10.0 * (v / frames as f64).log10()).collect();
        let mut sorted = db.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = sorted[n / 2];
        let mut peaks: Vec<(usize, f64)> = (2..n - 2)
            .filter(|&k| db[k] >= db[k - 1] && db[k] >= db[k + 1] && db[k] - med > 10.0 && (k as i64 - n as i64 / 2).abs() > 2)
            .map(|k| (k, db[k] - med))
            .collect();
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        // keying sidebands and leakage around a stronger carrier are not carriers of their own
        let near = (2000.0 / (fs / n as f64)) as i64;
        let mut kept: Vec<(usize, f64)> = Vec::new();
        for p in peaks {
            if kept.iter().all(|k| (k.0 as i64 - p.0 as i64).abs() > near) {
                kept.push(p);
            }
        }
        let peaks = kept;
        println!("noise     : {med:.1} dBFS per {:.0} Hz bin", fs / n as f64);
        println!("carriers  :");
        for (k, d) in peaks.iter().take(8) {
            let f = center + (*k as f64 - n as f64 / 2.0) * fs / n as f64;
            println!("            {:.6} MHz  {:+.1} kHz from the VFO  {:5.1} dB over the noise", f / 1e6, (f - vfo) / 1e3, d);
        }
    }
    // CW around the VFO: USB with the dial 1 kHz below it, so a carrier from 0.8 kHz below to
    // 2 kHz above the VFO gives an audible tone
    const DIAL: f64 = 1000.0;
    let mut d = Demodulator::new(Mode::Usb, fs, vfo - DIAL - center, 3000.0);
    let mut audio = Vec::new();
    for c in iq.chunks(16384) {
        d.process(c, &mut audio);
    }
    let r = sh_dsp::cw::decode(&audio, AUDIO_RATE, (200.0, 3000.0));
    println!("CW at VFO : {:?}", r.text);
    if !r.tone.is_empty() {
        println!("            {:.1} WPM, C/N0 {:.1} dB-Hz, carrier {:+.0} Hz from the VFO, drift {:+.2} Hz/s", r.wpm, r.cn0_dbhz, r.tone[0].1 - DIAL, r.drift_hz_s);
    }
    if let Some(out) = a.get(3) {
        // 8-bit greyscale waterfall, newest at the bottom, 10 rows/s
        let (lo, hi) = {
            let mut all: Vec<f32> = rows.iter().flatten().copied().collect();
            all.sort_by(|a, b| a.partial_cmp(b).unwrap());
            (all[all.len() / 5], all[all.len() * 999 / 1000])
        };
        let mut f = std::fs::File::create(out).expect("create");
        write!(f, "P5\n{} {}\n255\n", 1024, rows.len()).unwrap();
        for row in &rows {
            let px: Vec<u8> = row.iter().map(|v| (((v - lo) / (hi - lo)).clamp(0.0, 1.0) * 255.0) as u8).collect();
            f.write_all(&px).unwrap();
        }
        println!("waterfall : {out} ({} rows, {:.0} Hz per pixel)", rows.len(), fs / 1024.0);
    }
}
