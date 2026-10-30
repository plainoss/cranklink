package app.plainoss.cranklink.shared

/**
 * Bounded notification delivery at the Android GATT boundary. The caller owns synchronization.
 * A peer can close just after its final reply; only that terminal condition permits draining
 * complete queued packets. Overflow, oversized packets, failed IO and explicit close discard
 * the queue because it can no longer establish a complete protocol exchange.
 */
class BleNotificationBuffer(private val capacity: Int = 256) {
    init { require(capacity in 1..256) }
    private val packets = ArrayDeque<ByteArray>()
    private var failure: String? = null

    fun reset() {
        packets.clear()
        failure = null
    }

    /** Returns false after any terminal condition, overflow, or oversized notification. */
    fun enqueue(characteristic: Int, value: ByteArray): Boolean {
        require(characteristic in 0..65535)
        if (failure != null) return false
        if (value.size > 20 || packets.size >= capacity) {
            terminate("Bluetooth notification size or queue limit exceeded; reconnect")
            return false
        }
        packets.addLast(byteArrayOf(characteristic.toByte(), (characteristic shr 8).toByte()) + value)
        return true
    }

    fun terminate(reason: String, allowQueuedReplies: Boolean = false) {
        if (failure == null) failure = reason
        if (!allowQueuedReplies) packets.clear()
    }

    /** Null means an empty live queue; a drained terminal queue always throws. */
    fun poll(): ByteArray? {
        if (packets.isNotEmpty()) return packets.removeFirst()
        check(failure == null) { failure ?: "Bluetooth notification stream closed" }
        return null
    }
}
