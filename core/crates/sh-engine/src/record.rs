//! IQ recorder: the samples the DSP gets (after decimation) as a 2-channel 16-bit WAV, the
//! format SDR++ and most SDR programs play back. The file name follows SDR++
//! (`baseband_<Hz>Hz_<HH-MM-SS>_<DD-MM-YYYY>.wav`, so SDR++ shows the right frequency) and a
//! JSON chunk ("shmd", ignored by other readers) after the data keeps what the receiver knew:
//! satellite, station, mode, gain, ppm, every retune with its time.
//!
//! Writing happens on its own thread: the DSP only hands over a block, a slow card or phone
//! storage cannot stall the receiver (a full queue drops the block and counts it).

use serde_json::{json, Value};
use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::JoinHandle;

enum Msg {
    Data(Vec<i16>),
    Finish(Value),
}

pub struct Recorder {
    tx: SyncSender<Msg>,
    thread: Option<JoinHandle<std::io::Result<()>>>,
    pub name: String,
    pub rate: f64,
    pub start_unix: f64,
    frames: Arc<AtomicU64>,
    pub dropped: u64,
    retunes: Vec<(f64, f64)>,
    last_center: f64,
    meta: Value,
}

fn header(rate: u32, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&2u16.to_le_bytes()); // I, Q
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 4).to_le_bytes());
    h[32..34].copy_from_slice(&4u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// SDR++ style name for a recording starting now at `center_hz`.
pub fn file_name(center_hz: f64, unix: f64) -> String {
    let t = unix as i64;
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("baseband_{:.0}Hz_{:02}-{:02}-{:02}_{:02}-{:02}-{:04}.wav", center_hz, secs / 3600, secs % 3600 / 60, secs % 60, d, m, y)
}

impl Recorder {
    /// Start writing to `out` (a file, or on Android the Downloads entry's descriptor).
    pub fn start<W: Write + Seek + Send + 'static>(mut out: W, name: String, rate: f64, center_hz: f64, start_unix: f64, meta: Value) -> std::io::Result<Recorder> {
        out.write_all(&header(rate as u32, 0))?;
        let (tx, rx) = sync_channel::<Msg>(256);
        let frames = Arc::new(AtomicU64::new(0));
        let fr = frames.clone();
        let thread = std::thread::Builder::new().name("iq-recorder".into()).spawn(move || -> std::io::Result<()> {
            let mut w = std::io::BufWriter::with_capacity(1 << 20, out);
            let mut data_bytes: u64 = 0;
            let mut bytes = Vec::new();
            while let Ok(m) = rx.recv() {
                match m {
                    Msg::Data(v) => {
                        // WAV data is limited to 4 GB; stop adding samples there
                        if data_bytes + 2 * v.len() as u64 > u32::MAX as u64 - 1_000_000 {
                            continue;
                        }
                        bytes.clear();
                        bytes.extend(v.iter().flat_map(|s| s.to_le_bytes()));
                        w.write_all(&bytes)?;
                        data_bytes += bytes.len() as u64;
                        fr.store(data_bytes / 4, Ordering::Relaxed);
                    }
                    Msg::Finish(meta) => {
                        let mut js = serde_json::to_vec_pretty(&meta).unwrap_or_default();
                        if js.len() % 2 == 1 {
                            js.push(b' ');
                        }
                        w.write_all(b"shmd")?;
                        w.write_all(&(js.len() as u32).to_le_bytes())?;
                        w.write_all(&js)?;
                        let total = 4 + 8 + 16 + 8 + data_bytes + 8 + js.len() as u64;
                        let mut f = w.into_inner().map_err(|e| e.into_error())?;
                        f.seek(SeekFrom::Start(0))?;
                        let mut h = header(rate as u32, data_bytes as u32);
                        h[4..8].copy_from_slice(&(total as u32).to_le_bytes());
                        f.write_all(&h)?;
                        f.flush()?;
                        return Ok(());
                    }
                }
            }
            Ok(())
        })?;
        Ok(Recorder {
            tx,
            thread: Some(thread),
            name,
            rate,
            start_unix,
            frames,
            dropped: 0,
            retunes: vec![(0.0, center_hz)],
            last_center: center_hz,
            meta,
        })
    }

    /// Hand over a block (from the DSP thread). `center_hz`: the block's tuning; a change is
    /// logged with its time in the recording.
    pub fn push(&mut self, samples: &[sh_dsp::Complex32], center_hz: f64) {
        if (center_hz - self.last_center).abs() > 0.5 {
            self.retunes.push((self.seconds(), center_hz));
            self.last_center = center_hz;
        }
        let v: Vec<i16> = samples
            .iter()
            .flat_map(|c| [(c.re.clamp(-1.0, 1.0) * 32767.0) as i16, (c.im.clamp(-1.0, 1.0) * 32767.0) as i16])
            .collect();
        match self.tx.try_send(Msg::Data(v)) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => self.dropped += 1,
        }
    }

    pub fn seconds(&self) -> f64 {
        self.frames.load(Ordering::Relaxed) as f64 / self.rate
    }

    pub fn bytes(&self) -> u64 {
        44 + self.frames.load(Ordering::Relaxed) * 4
    }

    /// Close the file (blocks until everything is on disk).
    pub fn finish(mut self, extra: Value) -> Result<Value, String> {
        let mut meta = self.meta.clone();
        if let (Some(m), Some(e)) = (meta.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                m.insert(k.clone(), v.clone());
            }
        }
        meta["duration_s"] = json!(self.seconds());
        meta["dropped_blocks"] = json!(self.dropped);
        meta["retunes"] = json!(self.retunes.iter().map(|(t, f)| json!({"t_s": t, "center_hz": f})).collect::<Vec<_>>());
        let _ = self.tx.send(Msg::Finish(meta.clone()));
        let t = self.thread.take().ok_or("recorder already closed")?;
        t.join().map_err(|_| "recorder thread panicked".to_string())?.map_err(|e| e.to_string())?;
        Ok(meta)
    }
}

/// Read a recording back: (rate, IQ samples, metadata).
pub fn read_wav(bytes: &[u8]) -> Result<(f64, Vec<sh_dsp::Complex32>, Value), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let mut pos = 12;
    let (mut rate, mut bits, mut ch, mut fmt_tag) = (0u32, 0u16, 0u16, 0u16);
    let mut iq = Vec::new();
    let mut meta = Value::Null;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let len = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = &bytes[pos + 8..(pos + 8 + len).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => {
                fmt_tag = u16::from_le_bytes([body[0], body[1]]);
                ch = u16::from_le_bytes([body[2], body[3]]);
                rate = u32::from_le_bytes(body[4..8].try_into().unwrap());
                bits = u16::from_le_bytes([body[14], body[15]]);
            }
            b"data" => {
                if ch != 2 {
                    return Err(format!("{ch} channels, IQ needs 2"));
                }
                match (fmt_tag, bits) {
                    (1, 16) => iq.extend(body.chunks_exact(4).map(|b| {
                        sh_dsp::Complex32::new(i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0, i16::from_le_bytes([b[2], b[3]]) as f32 / 32768.0)
                    })),
                    (1, 8) => iq.extend(body.chunks_exact(2).map(|b| sh_dsp::Complex32::new((b[0] as f32 - 127.5) / 128.0, (b[1] as f32 - 127.5) / 128.0))),
                    (3, 32) => iq.extend(body.chunks_exact(8).map(|b| {
                        sh_dsp::Complex32::new(f32::from_le_bytes(b[0..4].try_into().unwrap()), f32::from_le_bytes(b[4..8].try_into().unwrap()))
                    })),
                    _ => return Err(format!("unsupported WAV format {fmt_tag}/{bits} bit")),
                }
            }
            b"shmd" => meta = serde_json::from_slice(body).unwrap_or(Value::Null),
            _ => {}
        }
        pos += 8 + len + (len & 1);
    }
    if rate == 0 {
        return Err("no fmt chunk".into());
    }
    Ok((rate as f64, iq, meta))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sh_dsp::{Complex32, Nco};

    #[test]
    fn name_like_sdrpp() {
        // 2026-11-01 10:41:19 UTC
        assert_eq!(file_name(437_310_000.0, 1_793_529_679.0), "baseband_437310000Hz_10-41-19_01-11-2026.wav");
    }

    #[test]
    fn round_trip_with_metadata_and_retunes() {
        let dir = std::env::temp_dir().join(format!("shrec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.wav");
        let f = std::fs::File::create(&path).unwrap();
        let rate = 300_000.0;
        let mut r = Recorder::start(f, "t.wav".into(), rate, 437.3e6, 0.0, json!({"satellite": "ITUPSAT 1"})).unwrap();
        let mut nco = Nco::new(-12_345.0, rate);
        let block: Vec<Complex32> = (0..30_000).map(|_| nco.next() * 0.5).collect();
        for k in 0..10 {
            r.push(&block, if k < 5 { 437.3e6 } else { 437.31e6 });
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let meta = r.finish(json!({"mode": "CW"})).unwrap();
        assert_eq!(meta["retunes"].as_array().unwrap().len(), 2);
        let (fs, iq, m) = read_wav(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(fs, rate);
        assert_eq!(iq.len(), 300_000);
        assert!((iq[1000] - block[1000]).norm() < 1e-3);
        assert_eq!(m["satellite"], "ITUPSAT 1");
        assert_eq!(m["mode"], "CW");
        assert!((m["duration_s"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        // header sizes are patched
        let b = std::fs::read(&path).unwrap();
        assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize, b.len() - 8);
        assert_eq!(u32::from_le_bytes(b[40..44].try_into().unwrap()), 1_200_000);
        std::fs::remove_dir_all(&dir).ok();
    }
}
