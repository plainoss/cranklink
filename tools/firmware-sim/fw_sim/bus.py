"""Deterministic delivery ordering; ticks are logical time, not CPU cycles."""
import heapq
from dataclasses import dataclass, field
from .machine import require

LANE_PRIMARY_CAPACITY = 15
LANE_SECONDARY_CAPACITY = 5


def packet_fragments_count(payload_length: int) -> int:
    """Calculates fragments required for a given payload length under D4.2.0 rules (0x32B34).
    
    1..3 bytes -> 1 fragment
    4..6 bytes -> 2 fragments
    7..9 bytes -> 3 fragments
    """
    if payload_length < 4:
        return 1
    elif payload_length < 7:
        return 2
    return 3


class FragmentLane:
    """Models D4.2.0 packet fragment queue admission logic (0x32B34)."""
    def __init__(self, capacity: int = LANE_PRIMARY_CAPACITY, name: str = 'lane'):
        self.capacity = capacity
        self.name = name
        self.count = 0
        self.admitted_packets = 0
        self.rejected_packets = 0

    def can_admit(self, fragments: int) -> bool:
        return (self.count + fragments) <= self.capacity

    def admit(self, fragments: int) -> bool:
        if not self.can_admit(fragments):
            self.rejected_packets += 1
            return False
        self.count += fragments
        self.admitted_packets += 1
        return True

    def release(self, fragments: int) -> None:
        self.count = max(0, self.count - fragments)


@dataclass(order=True)
class Event:
    deadline: int
    sequence: int
    direction: str = field(compare=False)
    packet: dict = field(compare=False)
    fault: str | None = field(default=None, compare=False)
    fragments: int = field(default=1, compare=False)
    lane: str = field(default='primary', compare=False)


class Bus:
    def __init__(self, gateway, motor):
        self.gateway, self.motor = gateway, motor
        self.time, self.sequence = 0, 0
        self.events, self.trace, self.replies = [], [], []
        self.drop_reply = False
        self.fragment_fault = None
        self.lane_primary = FragmentLane(LANE_PRIMARY_CAPACITY, 'primary')
        self.lane_secondary = FragmentLane(LANE_SECONDARY_CAPACITY, 'secondary')

    def admit_packet(self, packet, lane='primary'):
        target_lane = self.lane_primary if lane == 'primary' else self.lane_secondary
        payload = packet.get('payload', '')
        payload_len = len(bytes.fromhex(payload)) if isinstance(payload, str) else len(payload)
        frags = packet_fragments_count(payload_len)
        return target_lane.admit(frags)

    def enqueue(self, packet, direction='gateway->motor', delay=1, fault=None, lane='primary', check_lane=False):
        require(direction in ('gateway->motor','motor->gateway'), 'unknown bus direction')
        require(fault in (None, 'drop'), 'unknown bus fault')
        require(delay >= 0, 'negative event delay')
        target_lane = self.lane_primary if lane == 'primary' else self.lane_secondary
        payload = packet.get('payload', '')
        payload_len = len(bytes.fromhex(payload)) if isinstance(payload, str) else len(payload)
        frags = packet_fragments_count(payload_len)
        if check_lane:
            require(target_lane.admit(frags), f'fragment queue admission failed: {lane} lane full')
        self.sequence += 1
        heapq.heappush(self.events, Event(self.time + delay, self.sequence, direction, dict(packet), fault, frags, lane))

    def drain(self):
        delivered = 0
        while self.events:
            delivered += 1
            require(delivered <= 1000, 'event budget exhausted')
            event = heapq.heappop(self.events)
            target_lane = self.lane_primary if event.lane == 'primary' else self.lane_secondary
            target_lane.release(event.fragments)
            self.time = event.deadline
            self.trace.append(dict(time=self.time, sequence=event.sequence, direction=event.direction,
                                   **event.packet, fault=event.fault))
            if event.fault == 'drop': continue
            if event.direction == 'gateway->motor':
                packet = event.packet
                require(packet['target'] in (self.gateway.slot, 63), 'unroutable virtual destination')
                for reply in self.motor.receive(bytes.fromhex(packet['payload']), packet['source']):
                    require(reply['target'] == packet['source'], 'unexpected motor reply target')
                    self.enqueue(dict(source=self.gateway.slot, **reply), 'motor->gateway',
                                 fault='drop' if self.drop_reply else None)
            else:
                reply = self.gateway.receive(bytes.fromhex(event.packet['payload']), self.fragment_fault)
                if reply is not None: self.replies.append(reply)
        return self.replies
