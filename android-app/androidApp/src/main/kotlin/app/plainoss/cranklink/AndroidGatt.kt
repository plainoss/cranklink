package app.plainoss.cranklink

import android.annotation.SuppressLint
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.HandlerThread
import android.os.Looper
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock
import app.plainoss.cranklink.shared.BleNotificationBuffer

internal val ETUBE_SERVICE_UUID: UUID = UUID.fromString("000018ff-5348-494d-414e-4f5f424c4500")
private val CLIENT_CONFIGURATION_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
private const val ETUBE_UUID_SUFFIX = "-5348-494d-414e-4f5f424c4500"

/**
 * Exclusive, blocking GATT adapter for the Rust protocol session. All operations except [close]
 * and credential clearing run on its background executor; callbacks use a separate looper.
 * A timeout, lost connection, failed GATT operation, or notification overflow closes the link.
 * No passkey or protocol payload is logged or persisted.
 */
@SuppressLint("MissingPermission") // The activity owns the runtime permission request.
class AndroidGatt(
    context: Context,
    private val address: String,
    pin: String,
    private val onDisconnected: (String) -> Unit = {},
) {
    private val applicationContext = context.applicationContext
    private val main = Handler(Looper.getMainLooper())
    private val operations = ReentrantLock()
    private val state = ReentrantLock()
    private val changed = state.newCondition()
    private var activeGatt: BluetoothGatt? = null
    private var callbackThread: HandlerThread? = null
    private var connected = false
    private var ready = false
    private var permanentlyClosed = false
    private var failure: String? = "Bluetooth connection is closed"
    private val discovered = sortedMapOf<Int, BluetoothGattCharacteristic>()
    private val subscribed = mutableSetOf<Int>()
    private val notifications = BleNotificationBuffer()
    private var peerDisconnected = false
    private var credential: CharArray? = pin.toCharArray()
    private var pending: Pending? = null

    private enum class Operation { DISCOVER, READ, WRITE, SUBSCRIBE }
    private class Pending(val operation: Operation, val id: Int?) {
        var completed = false
        var value = byteArrayOf()
    }

    /** Connect and discover only the Shimano E-TUBE configuration service. */
    fun open() {
        requireBackgroundThread()
        operations.withLock { openLocked() }
    }

    /** A fresh GATT connection for protocol role changes; retain the in-memory passkey. */
    fun reopen() {
        requireBackgroundThread()
        operations.withLock {
            release(detach("Bluetooth connection is reopening", clearCredential = false))
            openLocked()
        }
    }

    private fun openLocked() {
        state.withLock { check(!permanentlyClosed) { "Bluetooth connection is closed; choose the bike again" } }
        if (state.withLock { ready && failure == null }) return
        try {
            val manager = applicationContext.getSystemService(BluetoothManager::class.java)
                ?: error("Bluetooth LE is unavailable on this device")
            val adapter = manager.adapter ?: error("Bluetooth LE is unavailable on this device")
            check(adapter.isEnabled) { "Turn on Bluetooth before connecting" }
            val device = adapter.getRemoteDevice(address)
            state.withLock {
                check(!permanentlyClosed) { "Bluetooth connection is closed; choose the bike again" }
                check(activeGatt == null) { "Bluetooth connection is already opening" }
                failure = null
                connected = false
                ready = false
                discovered.clear()
                subscribed.clear()
                notifications.reset()
                peerDisconnected = false
                pending = null
                val thread = HandlerThread("cranklink-gatt").also { it.start() }
                callbackThread = thread
                // Holding state until assignment prevents an early callback racing activeGatt.
                activeGatt = device.connectGatt(
                    applicationContext,
                    false,
                    callback,
                    BluetoothDevice.TRANSPORT_LE,
                    BluetoothDevice.PHY_LE_1M_MASK,
                    Handler(thread.looper),
                ) ?: error("Android could not create a Bluetooth connection")
            }
            awaitConnection()
            transact(Operation.DISCOVER) { it.discoverServices() }
            state.withLock {
                val service = requireConnectedLocked().getService(ETUBE_SERVICE_UUID)
                    ?: error("This device does not expose the E-TUBE configuration service")
                service.characteristics.forEach { characteristic ->
                    shortId(characteristic.uuid)?.let { id ->
                        check(discovered.put(id, characteristic) == null) {
                            "Ambiguous E-TUBE characteristic 0x${id.toString(16)}"
                        }
                    }
                }
                check(discovered.isNotEmpty()) { "No E-TUBE configuration characteristics found" }
                ready = true
            }
        } catch (error: Exception) {
            fail(error.message ?: "Bluetooth connection failed")
            throw error
        }
    }

    /** JSON contract consumed by JNI; short IDs are unsigned 16-bit integers. */
    fun characteristics(): String = state.withLock {
        requireReadyLocked()
        JSONArray().apply {
            discovered.forEach { (id, characteristic) ->
                val properties = characteristic.properties
                put(JSONObject().apply {
                    put("id", id)
                    put("read", properties and BluetoothGattCharacteristic.PROPERTY_READ != 0)
                    put("write", properties and BluetoothGattCharacteristic.PROPERTY_WRITE != 0)
                    put("write_without_response", properties and BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE != 0)
                    put("notify", properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0)
                    put("indicate", properties and BluetoothGattCharacteristic.PROPERTY_INDICATE != 0)
                })
            }
        }.toString()
    }

    fun read(id: Int): ByteArray = serialized {
        val characteristic = characteristic(id, BluetoothGattCharacteristic.PROPERTY_READ)
        transact(Operation.READ, id) { it.readCharacteristic(characteristic) }
    }

    fun write(id: Int, bytes: ByteArray, withoutResponse: Boolean) {
        serialized {
            val property = if (withoutResponse) BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE
                else BluetoothGattCharacteristic.PROPERTY_WRITE
            val characteristic = characteristic(id, property)
            val writeType = if (withoutResponse) BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
                else BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            val payload = bytes.copyOf()
            transact(Operation.WRITE, id) { gatt ->
                if (Build.VERSION.SDK_INT >= 33) {
                    gatt.writeCharacteristic(characteristic, payload, writeType) == BluetoothStatusCodes.SUCCESS
                } else {
                    @Suppress("DEPRECATION")
                    characteristic.writeType = writeType
                    @Suppress("DEPRECATION")
                    characteristic.value = payload
                    @Suppress("DEPRECATION")
                    gatt.writeCharacteristic(characteristic)
                }
            }
        }
    }

    /** Enable local delivery and await the remote CCCD acknowledgement before returning. */
    fun subscribe(id: Int) {
        serialized {
            if (state.withLock { id in subscribed }) return@serialized
            val characteristic = characteristic(
                id,
                BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_INDICATE,
            )
            val descriptor = characteristic.getDescriptor(CLIENT_CONFIGURATION_UUID)
                ?: error("E-TUBE characteristic 0x${id.toString(16)} has no notification descriptor")
            val value = if (characteristic.properties and BluetoothGattCharacteristic.PROPERTY_NOTIFY != 0)
                BluetoothGattDescriptor.ENABLE_NOTIFICATION_VALUE else BluetoothGattDescriptor.ENABLE_INDICATION_VALUE
            transact(Operation.SUBSCRIBE, id) { gatt ->
                if (!gatt.setCharacteristicNotification(characteristic, true)) false
                else if (Build.VERSION.SDK_INT >= 33) {
                    gatt.writeDescriptor(descriptor, value) == BluetoothStatusCodes.SUCCESS
                } else {
                    @Suppress("DEPRECATION")
                    descriptor.value = value
                    @Suppress("DEPRECATION")
                    gatt.writeDescriptor(descriptor)
                }
            }
            state.withLock {
                requireReadyLocked()
                subscribed.add(id)
            }
        }
    }

    /** Empty on timeout; otherwise two little-endian characteristic ID bytes followed by value. */
    fun receive(timeoutMs: Long): ByteArray = serialized {
        require(timeoutMs >= 0) { "Notification timeout must not be negative" }
        state.withLock {
            var remaining = TimeUnit.MILLISECONDS.toNanos(timeoutMs)
            while (true) {
                // The peer may close immediately after queueing its final complete reply.
                notifications.poll()?.let { return@withLock it }
                requireReadyLocked()
                if (remaining <= 0) return@withLock byteArrayOf()
                remaining = changed.awaitNanos(remaining)
            }
            @Suppress("UNREACHABLE_CODE")
            byteArrayOf()
        }
    }

    /** Available only in memory until [clearPasskey] or final [close]. */
    fun passkey(): String = state.withLock {
        val pin = credential ?: error("Enter the six-digit E-TUBE passkey and reconnect")
        check(pin.size == 6 && pin.all { it in '0'..'9' }) { "The E-TUBE passkey must contain six digits" }
        String(pin)
    }

    fun clearPasskey() = state.withLock {
        credential?.fill('\u0000')
        credential = null
    }

    /** Idempotent, non-blocking cancellation; wakes a pending operation or notification wait. */
    fun close() {
        val resources = state.withLock {
            permanentlyClosed = true
            detach("Bluetooth connection is closed", clearCredential = true)
        }
        release(resources)
    }

    private fun <T> serialized(block: () -> T): T {
        requireBackgroundThread()
        return operations.withLock {
            try {
                block()
            } catch (error: InterruptedException) {
                Thread.currentThread().interrupt()
                fail("Bluetooth operation was cancelled")
                throw IllegalStateException("Bluetooth operation was cancelled", error)
            }
        }
    }

    private fun characteristic(id: Int, allowed: Int): BluetoothGattCharacteristic = state.withLock {
        requireReadyLocked()
        val characteristic = discovered[id] ?: error("Missing E-TUBE characteristic 0x${id.toString(16)}")
        check(characteristic.properties and allowed != 0) { "Unsupported E-TUBE characteristic operation" }
        characteristic
    }

    private fun transact(operation: Operation, id: Int? = null, start: (BluetoothGatt) -> Boolean): ByteArray {
        try {
            return state.withLock {
                val gatt = requireConnectedLocked()
                check(pending == null) { "A Bluetooth operation is already pending" }
                val request = Pending(operation, id)
                pending = request
                check(start(gatt)) { "Android rejected the Bluetooth ${operation.name.lowercase()} operation" }
                var remaining = TimeUnit.SECONDS.toNanos(10)
                while (!request.completed) {
                    requireConnectedLocked()
                    check(remaining > 0) { "Bluetooth ${operation.name.lowercase()} timed out; reconnect" }
                    remaining = changed.awaitNanos(remaining)
                }
                // A confirmed operation can finish just before an immediate peer shutdown.
                // Failed/overflowed IO remains fenced; only a successful callback is retained.
                if (!peerDisconnected) requireConnectedLocked()
                pending = null
                request.value
            }
        } catch (error: Exception) {
            fail(error.message ?: "Bluetooth operation failed")
            throw error
        }
    }

    private fun awaitConnection() = state.withLock {
        var remaining = TimeUnit.SECONDS.toNanos(10)
        while (!connected) {
            check(failure == null) { failure ?: "Bluetooth connection failed" }
            check(remaining > 0) { "Bluetooth connection timed out; reconnect" }
            remaining = changed.awaitNanos(remaining)
        }
    }

    private fun requireConnectedLocked(): BluetoothGatt {
        check(failure == null && connected) { failure ?: "Bluetooth is not connected" }
        return checkNotNull(activeGatt) { "Bluetooth connection is closed" }
    }

    private fun requireReadyLocked() {
        requireConnectedLocked()
        check(ready) { "E-TUBE service discovery is incomplete" }
    }

    private data class Resources(val gatt: BluetoothGatt?, val thread: HandlerThread?)

    private fun detach(reason: String, clearCredential: Boolean, expected: BluetoothGatt? = null,
        allowQueuedReplies: Boolean = false, onlyIfActive: Boolean = false): Resources = state.withLock {
        if (expected != null && expected !== activeGatt) return@withLock Resources(null, null)
        if (onlyIfActive && activeGatt == null && callbackThread == null) return@withLock Resources(null, null)
        val resources = Resources(activeGatt, callbackThread)
        activeGatt = null
        callbackThread = null
        connected = false
        ready = false
        failure = reason
        peerDisconnected = allowQueuedReplies
        pending = null
        discovered.clear()
        subscribed.clear()
        notifications.terminate(reason, allowQueuedReplies)
        if (clearCredential) clearPasskey()
        changed.signalAll()
        resources
    }

    private fun release(resources: Resources) {
        resources.gatt?.let { gatt ->
            runCatching { gatt.disconnect() }
            runCatching { gatt.close() }
        }
        resources.thread?.quitSafely()
    }

    private fun fail(reason: String, expected: BluetoothGatt? = null, allowQueuedReplies: Boolean = false) {
        val resources = detach(reason, clearCredential = false, expected = expected,
            allowQueuedReplies = allowQueuedReplies, onlyIfActive = true)
        release(resources)
        // Notify only once for this connection. Voluntary close/reopen use detach directly.
        // Posting after release keeps UI callbacks outside the GATT state lock and callback looper.
        if (resources.gatt != null || resources.thread != null) main.post { onDisconnected(reason) }
    }

    private fun complete(gatt: BluetoothGatt, operation: Operation, id: Int?, status: Int, value: ByteArray = byteArrayOf()) {
        var error: String? = null
        state.withLock {
            if (gatt !== activeGatt) return
            if (status != BluetoothGatt.GATT_SUCCESS) {
                error = "Bluetooth ${operation.name.lowercase()} failed (GATT $status); reconnect"
            } else {
                pending?.takeIf { it.operation == operation && it.id == id }?.let { request ->
                    request.value = value.copyOf()
                    request.completed = true
                    changed.signalAll()
                }
            }
        }
        error?.let { fail(it, gatt) }
    }

    private fun notification(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) {
        var overflow = false
        state.withLock {
            if (gatt !== activeGatt || failure != null) return
            val id = shortId(characteristic.uuid) ?: return
            if (id !in discovered) return
            overflow = !notifications.enqueue(id, value)
            changed.signalAll()
        }
        if (overflow) fail("Bluetooth notification size or queue limit exceeded; reconnect", gatt)
    }

    private val callback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            val error = state.withLock {
                if (gatt !== activeGatt) return
                if (status == BluetoothGatt.GATT_SUCCESS && newState == BluetoothProfile.STATE_CONNECTED) {
                    connected = true
                    changed.signalAll()
                    null
                } else if (status != BluetoothGatt.GATT_SUCCESS || newState == BluetoothProfile.STATE_DISCONNECTED) {
                    "Bluetooth disconnected (GATT $status); reconnect"
                } else null
            }
            error?.let { fail(it, gatt, allowQueuedReplies = newState == BluetoothProfile.STATE_DISCONNECTED) }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) =
            complete(gatt, Operation.DISCOVER, null, status)

        override fun onCharacteristicRead(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray, status: Int) =
            complete(gatt, Operation.READ, shortId(characteristic.uuid), status, value)

        @Deprecated("Android API 26–32 callback")
        override fun onCharacteristicRead(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) {
            if (Build.VERSION.SDK_INT < 33) {
                @Suppress("DEPRECATION")
                complete(gatt, Operation.READ, shortId(characteristic.uuid), status, characteristic.value ?: byteArrayOf())
            }
        }

        override fun onCharacteristicWrite(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, status: Int) =
            complete(gatt, Operation.WRITE, shortId(characteristic.uuid), status)

        override fun onDescriptorWrite(gatt: BluetoothGatt, descriptor: BluetoothGattDescriptor, status: Int) {
            if (descriptor.uuid == CLIENT_CONFIGURATION_UUID) {
                complete(gatt, Operation.SUBSCRIBE, shortId(descriptor.characteristic.uuid), status)
            }
        }

        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic, value: ByteArray) =
            notification(gatt, characteristic, value)

        @Deprecated("Android API 26–32 callback")
        override fun onCharacteristicChanged(gatt: BluetoothGatt, characteristic: BluetoothGattCharacteristic) {
            if (Build.VERSION.SDK_INT < 33) {
                @Suppress("DEPRECATION")
                notification(gatt, characteristic, characteristic.value ?: byteArrayOf())
            }
        }
    }

    private fun requireBackgroundThread() {
        check(Looper.myLooper() != Looper.getMainLooper()) { "Run Bluetooth operations on the session executor" }
    }

    private fun shortId(uuid: UUID): Int? {
        val value = uuid.toString()
        if (!value.startsWith("0000") || !value.endsWith(ETUBE_UUID_SUFFIX)) return null
        return value.substring(4, 8).toIntOrNull(16)
    }
}
