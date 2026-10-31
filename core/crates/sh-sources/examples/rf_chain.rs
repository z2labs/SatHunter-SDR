//! RF chain report: `cargo run --release -p sh-sources --example rf_chain`
//! Link budget for a 70 cm CubeSat beacon, then the simulated RTL-SDR bytes through the app's
//! own conversion, spectrum and CW demodulator, old and new settings side by side.

use sh_sources::rfsim::*;
use std::time::Instant;

const FS: f64 = 2_400_000.0;
const F: f64 = 435.0e6;

fn main() {
    let on = Chain::typical(true);
    let off = Chain::typical(false);
    println!("== Noise budget (6-el Yagi, LNA 0.8 dB / 20 dB, 1 dB coax, RTL-SDR v3)");
    println!("T_sys LNA powered   {:7.0} K   N0 {:.1} dBm/Hz", on.t_sys_k(), on.n0_dbm_hz());
    println!("T_sys LNA unpowered {:7.0} K   N0 {:.1} dBm/Hz", off.t_sys_k(), off.n0_dbm_hz());
    let g_on = agc_gain(&on, FS);
    let g_off = agc_gain(&off, FS);
    println!("automatic gain settles at {g_on:.1} dB (LNA on), {g_off:.1} dB (LNA off); max {:.1}", max_gain());
    println!();
    println!("== Link budget at 435 MHz (10.5 dBi Yagi, 0 dBi on the satellite, 3 dB polarisation)");
    for (ptx, name) in [(20.0, "100 mW"), (27.0, "500 mW"), (30.0, "1 W")] {
        let row: Vec<String> = [600.0, 1200.0, 2000.0, 2500.0]
            .iter()
            .map(|r| {
                let p = link_dbm(ptx, 0.0, 10.5, F, *r, 3.0);
                format!("{r:5.0} km {p:7.1} dBm (C/N0 {:4.1} dB-Hz)", p - on.n0_dbm_hz())
            })
            .collect();
        println!("{name:7} {}", row.join("  "));
    }
    println!();
    println!("== Waterfall: carrier / noise peaks above the median noise, dB  (visible = carrier > peaks + 3)");
    println!("{:>9} {:>6} | {:^24} | {:^24} | {:^24} | {:^24}", "beacon", "LNA", "old 2048, 1 FFT/frame", "new 8192, averaged", "new 16384, averaged", "new 8192 + decim 8");
    for p in [-120.0, -130.0, -135.0, -140.0, -145.0] {
        for (lna, chain, g) in [("on", &on, g_on), ("off", &off, g_off)] {
            if lna == "off" && p < -130.0 {
                continue;
            }
            let mut rng = Rng::new(9);
            let mut ph = 0.0;
            let off_hz = 60_000.0;
            let raw = render_u8(chain, g, FS, Some(Carrier { p_dbm: p, offset_hz: off_hz }), (FS * 0.5) as usize, &mut ph, &mut rng);
            let (x, fs) = receive(&raw, FS, 1);
            let (y, fs8) = receive(&raw, FS, 8);
            let cells = [
                spectrum_visibility(&x, fs, 2048, off_hz, true),
                spectrum_visibility(&x, fs, 8192, off_hz, false),
                spectrum_visibility(&x, fs, 16384, off_hz, false),
                spectrum_visibility(&y, fs8, 8192, off_hz, false),
            ];
            let s: Vec<String> = cells
                .iter()
                .map(|v| format!("{:5.1}/{:4.1} {:>5.0}Hz {:3}", v.carrier_db, v.noise_peaks_db, v.bin_hz, if v.visible() { "YES" } else { "no" }))
                .collect();
            println!("{p:6.0}dBm {lna:>6} | {} ", s.join(" | "));
        }
    }
    println!();
    println!("== CW audio: tone vs noise in 100 Hz, dB, beacon -130 dBm, VFO on the database frequency");
    println!("{:>12} | {:>10} | {:>10}", "beacon off", "500 Hz", "2 kHz");
    for err in [0.0, 200.0, 400.0, 600.0, 900.0, -600.0] {
        let mut rng = Rng::new(3);
        let mut ph = 0.0;
        let vfo = 480_000.0;
        let raw = render_u8(&on, g_on, FS, Some(Carrier { p_dbm: -130.0, offset_hz: vfo + err }), (FS * 0.6) as usize, &mut ph, &mut rng);
        let (x, fs) = receive(&raw, FS, 1);
        let a = cw_audio_snr(&x, fs, vfo, vfo + err, 500.0);
        let b = cw_audio_snr(&x, fs, vfo, vfo + err, 2000.0);
        println!("{err:>9.0} Hz | {a:>10.1} | {b:>10.1}");
    }
    println!();
    println!("== CPU, this machine: DSP time for 1 s of 2.4 MS/s");
    let mut rng = Rng::new(5);
    let mut ph = 0.0;
    let raw = render_u8(&on, g_on, FS, None, FS as usize, &mut ph, &mut rng);
    for (name, fft, dec) in [("spectrum 8192 + CW demod", 8192, 1), ("spectrum 16384 + CW demod", 16384, 1), ("decim 8 + spectrum 8192 + CW", 8192, 8)] {
        let t = Instant::now();
        let (x, fs) = receive(&raw, FS, dec);
        let mut sp = sh_dsp::spectrum::Spectrum::new(fft, fs, 30.0, 0.4);
        sp.push(&x, |_| {});
        let mut d = sh_dsp::demod::Demodulator::new(sh_dsp::demod::Mode::Cw, fs, 60_000.0, 2000.0);
        let mut audio = Vec::new();
        for c in x.chunks(16384 / dec) {
            d.process(c, &mut audio);
        }
        println!("{name:30} {:6.1} ms", t.elapsed().as_secs_f64() * 1e3);
    }
}
