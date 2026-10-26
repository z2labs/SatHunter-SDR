//! USB access with `nusb`: on Android from the file descriptor the app's USB permission gave
//! us, on the desktop by vendor / product ID. Plus a small CDC-ACM serial port (the ESP32-S3's
//! USB Serial/JTAG) with queued bulk IN transfers so nothing is lost between reads.

use crate::SourceError;
use nusb::transfer::{Buffer, Bulk, ControlOut, ControlType, In, Out, Recipient};
use nusb::{Device, Endpoint, Interface, MaybeFuture};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Where a USB device comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsbId {
    pub vid: u16,
    pub pid: u16,
    /// Android: file descriptor from UsbDeviceConnection (we duplicate it)
    pub fd: Option<i32>,
}

pub const ESP32S3_JTAG: (u16, u16) = (0x303a, 0x1001);
pub const RTL_IDS: [(u16, u16); 2] = [(0x0bda, 0x2838), (0x0bda, 0x2832)];

pub fn is_rtl(vid: u16, pid: u16) -> bool {
    RTL_IDS.contains(&(vid, pid))
}

fn err(e: impl std::fmt::Display) -> SourceError {
    SourceError::Device(e.to_string())
}

pub fn open(id: &UsbId) -> Result<Device, SourceError> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    if let Some(fd) = id.fd {
        use std::os::fd::BorrowedFd;
        // SAFETY: the fd is open (owned by the Java UsbDeviceConnection); we only duplicate it
        let owned = unsafe { BorrowedFd::borrow_raw(fd) }.try_clone_to_owned().map_err(err)?;
        return Device::from_fd(owned).wait().map_err(err);
    }
    #[cfg(target_os = "android")]
    {
        // Android has no USB enumeration for apps: the device comes with its permission's fd
        Err(SourceError::NotFound)
    }
    #[cfg(not(target_os = "android"))]
    {
        let info = nusb::list_devices()
            .wait()
            .map_err(err)?
            .find(|d| d.vendor_id() == id.vid && d.product_id() == id.pid)
            .ok_or(SourceError::NotFound)?;
        info.open().wait().map_err(err)
    }
}

/// CDC-ACM serial port over bulk endpoints.
pub struct CdcPort {
    _device: Device,
    comm: Option<Interface>,
    comm_num: u8,
    _data: Interface,
    ep_in: Endpoint<Bulk, In>,
    ep_out: Endpoint<Bulk, Out>,
    rx: VecDeque<u8>,
    dead: bool,
}

const XFER: usize = 16384;
const QUEUED: usize = 8;

impl CdcPort {
    pub fn open(id: &UsbId) -> Result<CdcPort, SourceError> {
        let device = open(id)?;
        let cfg = device.active_configuration().map_err(err)?;
        let (mut comm_num, mut data_num, mut ep_in, mut ep_out) = (None, None, 0u8, 0u8);
        for alt in cfg.interface_alt_settings() {
            if alt.alternate_setting() != 0 {
                continue;
            }
            if alt.class() == 2 && comm_num.is_none() {
                comm_num = Some(alt.interface_number());
            }
            if alt.class() == 10 && data_num.is_none() {
                let (mut i, mut o) = (0, 0);
                for ep in alt.endpoints() {
                    if ep.transfer_type() != nusb::descriptors::TransferType::Bulk {
                        continue;
                    }
                    if ep.address() & 0x80 != 0 { i = ep.address() } else { o = ep.address() }
                }
                if i != 0 && o != 0 {
                    data_num = Some(alt.interface_number());
                    ep_in = i;
                    ep_out = o;
                }
            }
        }
        let data_num = data_num.ok_or_else(|| err("not a CDC-ACM serial device"))?;
        let comm = comm_num.and_then(|n| device.detach_and_claim_interface(n).wait().ok());
        let data = device.detach_and_claim_interface(data_num).wait().map_err(err)?;
        let mut ep_in: Endpoint<Bulk, In> = data.endpoint(ep_in).map_err(err)?;
        let ep_out: Endpoint<Bulk, Out> = data.endpoint(ep_out).map_err(err)?;
        for _ in 0..QUEUED {
            ep_in.submit(Buffer::new(XFER));
        }
        let mut p = CdcPort {
            _device: device,
            comm,
            comm_num: comm_num.unwrap_or(0),
            _data: data,
            ep_in,
            ep_out,
            rx: VecDeque::new(),
            dead: false,
        };
        // DTR and RTS together (no reset into the ROM bootloader on the S3's USB Serial/JTAG)
        let _ = p.set_lines(true, true);
        Ok(p)
    }

    /// CDC SET_CONTROL_LINE_STATE
    pub fn set_lines(&mut self, dtr: bool, rts: bool) -> Result<(), SourceError> {
        let Some(c) = &self.comm else { return Ok(()) };
        let v = (dtr as u16) | ((rts as u16) << 1);
        c.control_out(
            ControlOut { control_type: ControlType::Class, recipient: Recipient::Interface, request: 0x22, value: v, index: self.comm_num as u16, data: &[] },
            Duration::from_millis(500),
        )
        .wait()
        .map_err(err)
    }

    pub fn write(&mut self, s: &[u8]) -> Result<(), SourceError> {
        let c = self.ep_out.transfer_blocking(Buffer::from(s.to_vec()), Duration::from_secs(1));
        c.status.map_err(err)?;
        Ok(())
    }

    /// Wait up to `timeout` for data; returns the bytes read (empty on timeout). Err = unplugged.
    pub fn read(&mut self, out: &mut Vec<u8>, max: usize, timeout: Duration) -> Result<(), SourceError> {
        if self.rx.is_empty() && !self.dead {
            let end = Instant::now() + timeout;
            loop {
                let left = end.saturating_duration_since(Instant::now());
                match self.ep_in.wait_next_complete(left) {
                    Some(c) => {
                        let ok = c.status.is_ok();
                        self.rx.extend(&c.buffer[..c.actual_len.min(c.buffer.len())]);
                        if !ok && c.actual_len == 0 {
                            self.dead = true;
                            break;
                        }
                        let mut b = c.buffer;
                        b.clear();
                        b.set_requested_len(XFER);
                        self.ep_in.submit(b);
                        if !self.rx.is_empty() {
                            break;
                        }
                    }
                    None => break,
                }
                if Instant::now() >= end {
                    break;
                }
            }
        }
        if self.rx.is_empty() && self.dead {
            return Err(SourceError::Device("USB device gone".into()));
        }
        let n = max.min(self.rx.len());
        out.extend(self.rx.drain(..n));
        Ok(())
    }

    pub fn flush_input(&mut self) {
        self.rx.clear();
    }
}
