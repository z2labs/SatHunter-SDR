//! Signal sources: the demo (simulated RTL-SDR), RTL-SDR, ESP32-S3 ESP-SDR, rtl_tcp ...
//! Every source runs its own thread and pushes sample blocks into a bounded queue; a full
//! queue drops the block (counted) instead of stalling the USB transfers.

pub use sh_dsp::Complex32;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};

pub mod demo;
pub mod esp;
pub mod rtl;
pub mod usb;

#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    #[error("device not found")]
    NotFound,
    #[error("frequency {0:.0} Hz is out of range")]
    OutOfRange(f64),
    #[error("{0}")]
    Device(String),
}

/// What a source can do.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceInfo {
    pub id: &'static str,
    pub name: String,
    pub min_hz: f64,
    pub max_hz: f64,
    pub sample_rates: Vec<f64>,
    /// Gain steps in dB (empty = no manual gain)
    pub gains: Vec<f32>,
}

/// One block of samples with the tuning it was taken at.
#[derive(Debug, Clone)]
pub struct Block {
    pub center_hz: f64,
    pub rate: f64,
    pub samples: Vec<Complex32>,
}

/// Queue between a source thread and the engine.
#[derive(Clone)]
pub struct Sink {
    tx: SyncSender<Block>,
    dropped: Arc<AtomicU64>,
}

impl Sink {
    pub fn new(depth: usize) -> (Sink, Receiver<Block>, Arc<AtomicU64>) {
        let (tx, rx) = sync_channel(depth);
        let dropped = Arc::new(AtomicU64::new(0));
        (Sink { tx, dropped: dropped.clone() }, rx, dropped)
    }

    /// false = the engine is gone, the source should stop.
    pub fn push(&self, b: Block) -> bool {
        match self.tx.try_send(b) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                true
            }
            Err(TrySendError::Disconnected(_)) => false,
        }
    }
}

pub trait Source: Send {
    fn info(&self) -> SourceInfo;
    fn start(&mut self, sink: Sink) -> Result<(), SourceError>;
    fn stop(&mut self);
    fn running(&self) -> bool;
    /// Returns the frequency actually set.
    fn set_frequency(&mut self, hz: f64) -> Result<f64, SourceError>;
    fn frequency(&self) -> f64;
    fn set_sample_rate(&mut self, fs: f64) -> Result<(), SourceError>;
    fn sample_rate(&self) -> f64;
    /// None = automatic gain
    fn set_gain(&mut self, db: Option<f32>) -> Result<(), SourceError>;
    /// The receiver can power an LNA / active antenna through its input (bias tee)
    fn has_bias_tee(&self) -> bool {
        false
    }
    fn set_bias_tee(&mut self, _on: bool) -> Result<(), SourceError> {
        Err(SourceError::Device("this receiver has no bias tee".into()))
    }
    /// Frequency correction of the receiver's crystal, ppm (positive = it runs fast)
    fn set_ppm(&mut self, _ppm: f64) -> Result<(), SourceError> {
        Ok(())
    }
    /// Device details or the last error, for the UI
    fn status(&self) -> String {
        String::new()
    }
}

/// What the Cubesat Hunter says the selected satellite is sending right now (as received here,
/// Doppler included). The demo source puts this signal on the air.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SatSignal {
    pub freq: f64,
    pub width: f64, // linear transponder passband, Hz
    pub baud: f64,
    pub elevation: f64,
    pub range_km: f64,
    pub kind: SignalKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignalKind {
    #[default]
    Fm,
    Cw,
    Data,
    Linear,
}

pub type SharedSatSignal = Arc<Mutex<Option<SatSignal>>>;
