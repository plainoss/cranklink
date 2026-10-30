package app.plainoss.cranklink

import android.annotation.SuppressLint
import android.bluetooth.BluetoothManager
import android.bluetooth.le.BluetoothLeScanner
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.location.LocationManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.ParcelUuid
import android.provider.Settings

data class NearbyBike(val address: String, val name: String, val rssi: Int)

/** A foreground-only, ten-second scan limited to E-TUBE service advertisements. */
@SuppressLint("MissingPermission") // The activity requests scan/connect/location permissions first.
class BleScanner(
    context: Context,
    private val onResults: (List<NearbyBike>) -> Unit,
    private val onError: (String) -> Unit,
) {
    private val applicationContext = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val bikes = mutableMapOf<String, NearbyBike>()
    private var scanner: BluetoothLeScanner? = null
    private var callback: ScanCallback? = null
    private var generation = 0L
    private val timeout = Runnable { stopOnMain() }

    /** Results and errors always arrive on the main thread. Repeated start while scanning is safe. */
    fun start() {
        if (Looper.myLooper() != Looper.getMainLooper()) {
            main.post { start() }
            return
        }
        if (callback != null) return
        try {
            val adapter = applicationContext.getSystemService(BluetoothManager::class.java)?.adapter
                ?: error("Bluetooth LE is unavailable on this device")
            check(adapter.isEnabled) { "Turn on Bluetooth before scanning" }
            // Before Android 12, BLE scans return nothing while location services are off.
            check(Build.VERSION.SDK_INT >= Build.VERSION_CODES.S || locationEnabled()) {
                "Turn on Location before scanning; Android requires it for Bluetooth scans on this version"
            }
            val leScanner = adapter.bluetoothLeScanner ?: error("Bluetooth LE scanning is unavailable")
            val token = ++generation
            bikes.clear()
            val scanCallback = object : ScanCallback() {
                override fun onScanResult(callbackType: Int, result: ScanResult) = publish(token, listOf(result))
                override fun onBatchScanResults(results: MutableList<ScanResult>) = publish(token, results)
                override fun onScanFailed(errorCode: Int) {
                    main.post {
                        if (generation == token && callback === this) {
                            stopOnMain()
                            onError("Bluetooth scan failed ($errorCode)")
                        }
                    }
                }
            }
            scanner = leScanner
            callback = scanCallback
            leScanner.startScan(
                listOf(ScanFilter.Builder().setServiceUuid(ParcelUuid(ETUBE_SERVICE_UUID)).build()),
                ScanSettings.Builder().setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY).build(),
                scanCallback,
            )
            main.postDelayed(timeout, 10_000)
            onResults(emptyList())
        } catch (error: Exception) {
            stopOnMain()
            onError(error.message ?: "Could not start Bluetooth scan")
        }
    }

    private fun locationEnabled(): Boolean =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            applicationContext.getSystemService(LocationManager::class.java)?.isLocationEnabled == true
        } else {
            @Suppress("DEPRECATION")
            Settings.Secure.getInt(applicationContext.contentResolver, Settings.Secure.LOCATION_MODE,
                Settings.Secure.LOCATION_MODE_OFF) != Settings.Secure.LOCATION_MODE_OFF
        }

    /** Call when leaving the discovery screen or when its activity stops. */
    fun stop() {
        if (Looper.myLooper() == Looper.getMainLooper()) stopOnMain()
        else main.post { stopOnMain() }
    }

    private fun stopOnMain() {
        main.removeCallbacks(timeout)
        val oldCallback = callback
        callback = null
        ++generation
        if (oldCallback != null) runCatching { scanner?.stopScan(oldCallback) }
        scanner = null
    }

    private fun publish(token: Long, results: List<ScanResult>) {
        main.post {
            if (generation != token || callback == null) return@post
            try {
                results.forEach { result ->
                    val address = result.device.address
                    val name = result.scanRecord?.deviceName ?: result.device.name ?: "E-TUBE bike"
                    bikes[address] = NearbyBike(address, name, result.rssi)
                }
                onResults(bikes.values.sortedByDescending { it.rssi })
            } catch (error: SecurityException) {
                stopOnMain()
                onError("Bluetooth permission was revoked; allow Nearby devices and scan again")
            }
        }
    }
}
