# Local changes (SatHunterSDR)

Vendored from rtl-sdr-rs 0.4.2 (https://github.com/ccostes/rtl-sdr-rs), MPL-2.0 (see LICENSE).

- `src/lib.rs`, `src/device/device_handle.rs`: no `nusb::list_devices()` on Android (nusb has no
  enumeration there); Android opens devices with `DeviceId::Fd` only.
