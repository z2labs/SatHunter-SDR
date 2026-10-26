package io.z2labs.sathunter

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbManager
import android.os.Build
import android.util.Log

/**
 * USB SDRs (RTL-SDR, ESP32-S3 ESP-SDR): asks for permission, opens the device and hands the
 * file descriptor to the Rust core, which drives it with nusb. Plug-and-play: plugging in
 * switches to the SDR, unplugging goes back to the demo source.
 */
class UsbSdrs(private val context: Context, private val engine: Engine) {
    private val um = context.getSystemService(Context.USB_SERVICE) as UsbManager
    private val open = HashMap<String, UsbDeviceConnection>()
    private val action = context.packageName + ".USB_PERMISSION"

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(c: Context, intent: Intent) {
            val dev: UsbDevice? = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
            else @Suppress("DEPRECATION") intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
            dev ?: return
            when (intent.action) {
                action -> if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false)) openDevice(dev)
                UsbManager.ACTION_USB_DEVICE_ATTACHED -> request(dev)
                UsbManager.ACTION_USB_DEVICE_DETACHED -> closeDevice(dev)
            }
        }
    }

    fun register() {
        val f = IntentFilter(action).apply {
            addAction(UsbManager.ACTION_USB_DEVICE_ATTACHED)
            addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
        }
        if (Build.VERSION.SDK_INT >= 33) context.registerReceiver(receiver, f, Context.RECEIVER_EXPORTED)
        else context.registerReceiver(receiver, f)
    }

    fun unregister() {
        try { context.unregisterReceiver(receiver) } catch (_: Exception) {}
        for (c in open.values) c.close()
        open.clear()
    }

    /** Devices already plugged in at start; true if one is permitted already (it starts at once). */
    fun scan(): Boolean {
        var ready = false
        for (d in um.deviceList.values) if (supported(d)) {
            if (um.hasPermission(d)) ready = true
            request(d)
        }
        return ready
    }

    fun handleIntent(intent: Intent?) {
        if (intent?.action != UsbManager.ACTION_USB_DEVICE_ATTACHED) return
        val dev: UsbDevice? = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(UsbManager.EXTRA_DEVICE, UsbDevice::class.java)
        else @Suppress("DEPRECATION") intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
        dev?.let { request(it) }
    }

    private fun supported(d: UsbDevice) =
        (d.vendorId == 0x303a && d.productId == 0x1001) || (d.vendorId == 0x0bda && (d.productId == 0x2838 || d.productId == 0x2832))

    private fun request(d: UsbDevice) {
        if (!supported(d)) return
        if (um.hasPermission(d)) {
            openDevice(d)
            return
        }
        // FLAG_MUTABLE: the system adds the device and the result to the intent
        val flags = if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0
        val pi = PendingIntent.getBroadcast(context, 0, Intent(action).setPackage(context.packageName), flags)
        um.requestPermission(d, pi)
    }

    private fun openDevice(d: UsbDevice) {
        if (open.containsKey(d.deviceName)) return
        val c = um.openDevice(d) ?: run { Log.w("SatHunter", "cannot open ${d.deviceName}"); return }
        open[d.deviceName] = c
        Log.i("SatHunter", "USB %04x:%04x fd %d".format(d.vendorId, d.productId, c.fileDescriptor))
        engine.usbAttach(c.fileDescriptor, d.vendorId, d.productId)
    }

    private fun closeDevice(d: UsbDevice) {
        engine.usbDetach(d.vendorId, d.productId)
        open.remove(d.deviceName)?.close()
    }
}
