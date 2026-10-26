# Local changes (SatHunterSDR)

Vendored from rtl-sdr-rs 0.4.2 (https://github.com/ccostes/rtl-sdr-rs), MPL-2.0 (see LICENSE).

- `src/lib.rs`, `src/device/device_handle.rs`: no `nusb::list_devices()` on Android (nusb has no
  enumeration there); Android opens devices with `DeviceId::Fd` only.
- `src/lib.rs`, `src/rtlsdr.rs`: `set_blog_v4_hint()` — the app can say the device is an RTL-SDR
  Blog V4 (Android knows the USB product strings even when reading them over the fd fails);
  `is_blog_v4()` reports what was chosen; the USB strings are logged.

- `AsyncReadConfigChange::BiasTee(bool)` and `AsyncReadControlHandle::set_bias_tee()`: switch the
  bias tee while the async reader owns the device.
