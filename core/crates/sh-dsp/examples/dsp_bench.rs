//! DSP cost per second of input: `cargo run --release -p sh-dsp --example dsp_bench`
use sh_dsp::demod::{Demodulator, Mode};
use sh_dsp::filter::Decimator;
use sh_dsp::spectrum::Spectrum;
use sh_dsp::{Complex32, Nco};
use std::time::Instant;

fn main() {
    let fs = 2_400_000.0;
    let mut nco = Nco::new(123_456.0, fs);
    let x: Vec<Complex32> = (0..fs as usize).map(|_| nco.next() * 0.1).collect();
    let t = |name: &str, f: &mut dyn FnMut()| {
        let s = Instant::now();
        f();
        println!("{name:34} {:6.1} ms", s.elapsed().as_secs_f64() * 1e3);
    };
    t("NCO alone (2.4 M sin_cos)", &mut || {
        let mut n = Nco::new(60_000.0, fs);
        let mut acc = Complex32::new(0.0, 0.0);
        for v in &x {
            acc += *v * n.next();
        }
        std::hint::black_box(acc);
    });
    for fft in [2048, 8192, 16384] {
        t(&format!("spectrum {fft} averaged"), &mut || {
            let mut sp = Spectrum::new(fft, fs, 30.0, 0.4);
            for c in x.chunks(16384) {
                sp.push(c, |_| {});
            }
        });
    }
    for (m, bw) in [(Mode::Cw, 2000.0), (Mode::Nfm, 16_000.0), (Mode::Wfm, 180_000.0)] {
        t(&format!("demod {}", m.name()), &mut || {
            let mut d = Demodulator::new(m, fs, 60_000.0, bw);
            let mut a = Vec::new();
            for c in x.chunks(16384) {
                d.process(c, &mut a);
            }
        });
    }
    for dec in [2, 4, 8] {
        t(&format!("decimator /{dec}"), &mut || {
            let mut d = Decimator::new(dec, fs);
            let mut o = Vec::new();
            for c in x.chunks(16384) {
                d.process(c, &mut o);
            }
        });
    }
}
