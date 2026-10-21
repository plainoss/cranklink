"""Virtual-time boundaries must not turn a zero-time drain into a future ACK."""
from collections import deque
import unittest

from d_relay_peer import Peer


class DRelayClockTests(unittest.TestCase):
    def peer(self):
        peer = Peer.__new__(Peer)
        peer.now = 0.0
        peer.events = []
        peer.event_sequence = 0
        peer.queue = deque()
        peer.stream_start = None
        peer.buffer_reads = set()
        return peer

    def test_zero_time_drain_cannot_consume_future_buffer_acceptance(self):
        peer = self.peer()
        peer.schedule(10, 'notify', b'\x2b\x00\x82\x02\x00')
        self.assertIsNone(peer.request(dict(op='receive', timeout_ms=0))['notification'])
        self.assertEqual(peer.now, 0)
        self.assertFalse(peer.buffer_reads)
        self.assertIsNone(peer.request(dict(op='receive', timeout_ms=9))['notification'])
        self.assertEqual(peer.now, 9)
        notification = peer.request(dict(op='receive', timeout_ms=2))['notification']
        self.assertEqual(notification['value'], [0x2b, 0, 0x82, 2, 0])
        self.assertEqual(peer.now, 10)
        self.assertEqual(peer.buffer_reads, {2})

    def test_equal_time_notifications_keep_order_and_all_run_at_deadline(self):
        peer = self.peer()
        for packet in (2, 4, 6):
            peer.schedule(5, 'notify', bytes([0x2b, 0, 0x82, packet, 0]))
        peer.advance(5)
        self.assertEqual([value[3] for value in peer.queue], [2, 4, 6])
        self.assertFalse(peer.events)


if __name__ == '__main__':
    unittest.main()
