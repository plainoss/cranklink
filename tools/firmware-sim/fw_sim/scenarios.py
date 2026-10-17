from .profiles import identify
from .machine import require
from .gateway import Gateway
from .motor import Motor
from .bus import Bus

FAULTS = ('none', 'drop-key', 'duplicate-key', 'reorder-key', 'delay-key', 'drop-reply',
          'truncate', 'reorder', 'corrupt-sequence')


def setup(gateway_path, motor_path, slot=13):
    gi, gp = identify(gateway_path, 'gateway')
    mi, mp = identify(motor_path, 'motor')
    return Gateway(gi, gp, slot), Motor(mi, mp)


def run(gateway_path, motor_path, fault='none', slot=13):
    require(fault in FAULTS, 'unknown fault')
    gateway, motor = setup(gateway_path, motor_path, slot)
    bus = Bus(gateway, motor)
    keys = gateway.start()
    require(len(keys) == 6, 'expected start and five keys')
    if fault == 'reorder-key': keys[1], keys[2] = keys[2], keys[1]
    for i, packet in enumerate(keys):
        bus.enqueue(packet, delay=i+1+(100 if fault=='delay-key' and i==5 else 0),
                    fault='drop' if fault=='drop-key' and i==5 else None)
        if fault == 'duplicate-key' and i == 1: bus.enqueue(packet, delay=i+1)
    bus.drain()
    connected = motor.connected
    require(connected == (fault not in ('drop-key', 'reorder-key')), 'unexpected handshake result')
    reads = []
    if connected:
        require(motor.byte(motor.profile['owner_ram']) == 0, 'PC owner is not zero')
        require(motor.byte(motor.profile['auth_ram']) == 0, 'PC link incorrectly grants privileged auth')
        # Exercise multiple replies with the same RAM/CPU instances: pending
        # scheduler entries must be consumed rather than returning stale data.
        for request in ('16ac01', '101400', '167c00'):
            bus.drop_reply = fault == 'drop-reply'
            bus.fragment_fault = fault if fault in ('truncate', 'reorder', 'corrupt-sequence') else None
            before = len(bus.replies)
            bus.enqueue(gateway.send(bytes.fromhex(request)))
            bus.drain()
            replies = bus.replies[before:]
            # Reordering a single-fragment lamp reply is a no-op.
            if fault in ('drop-reply', 'truncate', 'corrupt-sequence') or (fault == 'reorder' and request != '101400'):
                require(not replies, 'incomplete/corrupt/dropped response reported as success')
            else:
                require(len(replies) == 1, 'missing or duplicate normal response')
                expected = bytes([int(request[:2],16), int(request[2:4],16)+2])
                require(bytes.fromhex(replies[0]['payload']).startswith(expected), 'stale or wrong opcode reply')
            reads.append(dict(request=request, replies=replies))
    bus.drop_reply, bus.fragment_fault = False, None
    for packet in gateway.link_stop(): bus.enqueue(packet)
    bus.drain()
    require(not motor.connected, 'motor did not stop')
    gateway.restore()
    return dict(gateway=gateway.profile['id'], motor=motor.profile['id'], fault=fault,
                slot=slot, connected=connected, reads=reads, cleanup=True,
                modeled_peripheral=gateway.profile.get('peripheral_model'),
                peripheral_services=gateway.peripheral_services,
                trace=bus.trace, instructions=dict(gateway=gateway.instructions, motor=motor.instructions))
