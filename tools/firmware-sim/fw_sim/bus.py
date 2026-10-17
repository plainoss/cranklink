"""Deterministic delivery ordering; ticks are logical time, not CPU cycles."""
import heapq
from dataclasses import dataclass, field
from .machine import require


@dataclass(order=True)
class Event:
    deadline: int
    sequence: int
    direction: str = field(compare=False)
    packet: dict = field(compare=False)
    fault: str | None = field(default=None, compare=False)


class Bus:
    def __init__(self, gateway, motor):
        self.gateway, self.motor = gateway, motor
        self.time, self.sequence = 0, 0
        self.events, self.trace, self.replies = [], [], []
        self.drop_reply = False
        self.fragment_fault = None

    def enqueue(self, packet, direction='gateway->motor', delay=1, fault=None):
        require(direction in ('gateway->motor','motor->gateway'), 'unknown bus direction')
        require(fault in (None, 'drop'), 'unknown bus fault')
        require(delay >= 0, 'negative event delay')
        self.sequence += 1
        heapq.heappush(self.events, Event(self.time + delay, self.sequence, direction, dict(packet), fault))

    def drain(self):
        delivered = 0
        while self.events:
            delivered += 1
            require(delivered <= 1000, 'event budget exhausted')
            event = heapq.heappop(self.events)
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
