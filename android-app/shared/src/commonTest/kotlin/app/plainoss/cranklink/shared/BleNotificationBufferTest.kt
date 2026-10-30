package app.plainoss.cranklink.shared

import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class BleNotificationBufferTest {
    @Test fun drainsCompleteRepliesBeforeReportingPeerShutdown() {
        val buffer = BleNotificationBuffer()
        assertTrue(buffer.enqueue(0x2af9, byteArrayOf(0x10, 0)))
        assertTrue(buffer.enqueue(0x2afd, byteArrayOf(13, 1, 0x3e)))
        buffer.terminate("Bluetooth peer disconnected", allowQueuedReplies = true)
        assertContentEquals(byteArrayOf(0xf9.toByte(), 0x2a, 0x10, 0), buffer.poll())
        assertContentEquals(byteArrayOf(0xfd.toByte(), 0x2a, 13, 1, 0x3e), buffer.poll())
        assertFailsWith<IllegalStateException> { buffer.poll() }
        assertFalse(buffer.enqueue(0x2af9, byteArrayOf(1)))
    }

    @Test fun failedIoAndExplicitCloseNeverReturnEarlierPackets() {
        for (reason in listOf("GATT write failed", "Bluetooth operation timed out", "Connection closed")) {
            val buffer = BleNotificationBuffer()
            buffer.enqueue(0x2af9, byteArrayOf(0x10))
            buffer.terminate(reason)
            assertEquals(reason, assertFailsWith<IllegalStateException> { buffer.poll() }.message)
        }
        val buffer = BleNotificationBuffer()
        buffer.enqueue(0x2af9, byteArrayOf(0x10))
        buffer.terminate("Peer disconnected", allowQueuedReplies = true)
        buffer.terminate("Explicit close")
        assertFailsWith<IllegalStateException> { buffer.poll() }
    }

    @Test fun incompleteQueuesCannotReturnAFinalAcknowledgement() {
        val buffer = BleNotificationBuffer(2)
        repeat(2) { assertTrue(buffer.enqueue(0x2af9, byteArrayOf(it.toByte()))) }
        assertFalse(buffer.enqueue(0x2af9, byteArrayOf(0x10)))
        buffer.terminate("Peer disconnected", allowQueuedReplies = true)
        assertFailsWith<IllegalStateException> { buffer.poll() }
        val oversized = BleNotificationBuffer()
        oversized.enqueue(0x2af9, byteArrayOf(0x10))
        assertFalse(oversized.enqueue(0x2afd, ByteArray(21)))
        assertFailsWith<IllegalStateException> { oversized.poll() }
    }

    @Test fun reconnectStartsEmptyAndCopiesIncomingPacketValues() {
        val buffer = BleNotificationBuffer()
        buffer.enqueue(0x2af9, byteArrayOf(0x10))
        buffer.terminate("Peer disconnected", allowQueuedReplies = true)
        buffer.reset()
        assertNull(buffer.poll())
        val payload = byteArrayOf(7)
        buffer.enqueue(0x2afd, payload)
        payload[0] = 99
        assertContentEquals(byteArrayOf(0xfd.toByte(), 0x2a, 7), buffer.poll())
        assertNull(buffer.poll())
    }

    @Test fun productionQueueHasAnExactBoundOf256Packets() {
        val buffer = BleNotificationBuffer()
        repeat(256) { assertTrue(buffer.enqueue(0x2af9, ByteArray(20))) }
        assertFalse(buffer.enqueue(0x2af9, byteArrayOf(0x10)))
        assertFailsWith<IllegalStateException> { buffer.poll() }
        assertFailsWith<IllegalArgumentException> { BleNotificationBuffer(257) }
    }
}
