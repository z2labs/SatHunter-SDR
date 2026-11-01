//! Decode CW from a recording and report what was heard.
//! `cargo run --release -p sh-dsp --example cw_decode -- audio.f32 [rate]`
//! Input: raw mono 32-bit float samples (e.g. `ffmpeg -i rec.mp4 -vn -ac 1 -ar 48000 -f f32le audio.f32`).
use sh_dsp::cw::decode;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("usage: cw_decode audio.f32 [rate]");
    let fs: f64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(48_000.0);
    let bytes = std::fs::read(path).expect("read");
    let x: Vec<f32> = bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect();
    let r = decode(&x, fs, (300.0, 3500.0));
    println!("text      : {}", r.text);
    println!("speed     : {:.1} WPM (dot {:.0} ms)", r.wpm, r.dot_ms);
    println!("signal    : {:.1} dB over the noise in {:.0} Hz  =  C/N0 {:.1} dB-Hz", r.snr_db, 4.5 * fs / 2048.0, r.cn0_dbhz);
    println!("key down  : {:.0}% of the time", r.duty * 100.0);
    if let (Some(a), Some(b)) = (r.tone.first(), r.tone.last()) {
        println!("tone      : {:.1} Hz at {:.1} s -> {:.1} Hz at {:.1} s, drift {:+.2} Hz/s", a.1, a.0, b.1, b.0, r.drift_hz_s);
    }
}
