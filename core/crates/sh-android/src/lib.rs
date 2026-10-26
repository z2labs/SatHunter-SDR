//! JNI bridge for the Android app (class io.z2labs.sathunter.Native).
//! Every entry point catches panics: a bug in the core shows as an error, never as a crash.

use jni::objects::{JClass, JDoubleArray, JFloatArray, JString};
use jni::sys::{jint, jstring};
use jni::JNIEnv;
use sh_engine::Engine;
use std::sync::OnceLock;

static ENGINE: OnceLock<Engine> = OnceLock::new();

fn guard<T>(default: T, f: impl FnOnce() -> T) -> T {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(default)
}

#[no_mangle]
pub extern "system" fn Java_io_z2labs_sathunter_Native_init(mut env: JNIEnv, _c: JClass, dir: JString) {
    #[cfg(target_os = "android")]
    android_logger::init_once(android_logger::Config::default().with_max_level(log::LevelFilter::Info).with_tag("SatHunter"));
    let dir: String = match env.get_string(&dir) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    guard((), || {
        ENGINE.get_or_init(|| Engine::new(std::path::Path::new(&dir)));
    });
}

#[no_mangle]
pub extern "system" fn Java_io_z2labs_sathunter_Native_command<'a>(mut env: JNIEnv<'a>, _c: JClass, json: JString<'a>) -> jstring {
    let input: String = env.get_string(&json).map(|s| s.into()).unwrap_or_default();
    let out = guard(r#"{"ok":false,"error":"internal error"}"#.to_string(), || match ENGINE.get() {
        Some(e) => e.command(&input),
        None => r#"{"ok":false,"error":"engine not started"}"#.to_string(),
    });
    env.new_string(out).map(|s| s.into_raw()).unwrap_or(std::ptr::null_mut())
}

/// Copies new spectrum rows into `out` (rows * bins floats) and each row's centre frequency and
/// sample rate into `meta` (2 doubles per row). Returns rows << 16 | bins / 16 (bins is a
/// multiple of 16), 0 when nothing is new.
#[no_mangle]
pub extern "system" fn Java_io_z2labs_sathunter_Native_spectrumRows(env: JNIEnv, _c: JClass, out: JFloatArray, meta: JDoubleArray) -> jint {
    let Some(e) = ENGINE.get() else { return 0 };
    let len = env.get_array_length(&out).unwrap_or(0).max(0) as usize;
    let mlen = env.get_array_length(&meta).unwrap_or(0).max(0) as usize;
    let mut buf = vec![0.0f32; len];
    let mut mbuf = vec![0.0f64; mlen.max(2)];
    let (n, bins) = guard((0, 0), || e.spectrum_rows(&mut buf, &mut mbuf));
    if n > 0 {
        let _ = env.set_float_array_region(&out, 0, &buf[..n * bins]);
        let _ = env.set_double_array_region(&meta, 0, &mbuf[..(2 * n).min(mlen)]);
        return ((n as jint) << 16) | ((bins / 16) as jint);
    }
    0
}

/// Pulls 48 kHz mono audio into `out`, returns the sample count.
#[no_mangle]
pub extern "system" fn Java_io_z2labs_sathunter_Native_audio(env: JNIEnv, _c: JClass, out: JFloatArray) -> jint {
    let Some(e) = ENGINE.get() else { return 0 };
    let len = env.get_array_length(&out).unwrap_or(0).max(0) as usize;
    let mut buf = vec![0.0f32; len];
    let n = guard(0, || e.audio(&mut buf));
    if n > 0 {
        let _ = env.set_float_array_region(&out, 0, &buf[..n]);
    }
    n as jint
}
