#!/usr/bin/env python3
"""JSONL test boundary for the current browser app and exact component firmware.

Only GATT/radio, initialized RAM and physical NVM/actuator boundaries are
modeled. Gateway routing, PC proofs and component GET/SET/replies execute stock
instructions. MU actuator completion is deliberately absent. Firmware remains
local and hash-fenced by the existing fixtures; snapshots omit proof material.
"""
import argparse
import json
import sys
from pathlib import Path

from browser_peer import BrowserPeer
from fw_sim.component_gatt import ComponentGattDevice
from fw_sim.component_mu import MuUr500
from fw_sim.components_display import Display
from fw_sim.machine import require


class ComponentBrowserPeer(BrowserPeer):
    def __init__(self, gateway, component, kind, state=None):
        self.kind = kind
        super().__init__(gateway, component, state)

    def _new_device(self):
        if self.kind == 'display':
            target = Display.from_path(self.motor_path, allow_writes=True, storage_path=self.state_path)
        else:
            target = MuUr500(self.motor_path.read_bytes(), allow_saves=True, storage_path=self.state_path)
        return ComponentGattDevice(self.gateway_path, target)

    def request(self, message):
        reply = super().request(message)
        if message.get('op') in ('snapshot', 'power_cycle'):
            component = self.device.motor
            reply['result']['component_profile'] = component.profile['id']
            reply['result']['component_sha256'] = component.profile['sha256']
            reply['result']['gateway_sha256'] = self.device.gateway.profile['sha256']
            reply['result']['component_instructions'] = component.instructions
            if isinstance(component, Display):
                reply['result']['component_storage'] = component.snapshot()
            else:
                reply['result']['component_storage'] = component.storage.snapshot()
        return reply


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gateway', required=True, type=Path)
    parser.add_argument('--component', required=True, type=Path)
    parser.add_argument('--kind', required=True, choices=('display', 'mu'))
    parser.add_argument('--state', required=True, type=Path)
    args = parser.parse_args()
    peer = ComponentBrowserPeer(args.gateway, args.component, args.kind, args.state)
    try:
        while line := sys.stdin.buffer.readline(4097):
            message = None
            try:
                require(len(line) <= 4096 and line.endswith(b'\n'), 'browser peer request line exceeds limit')
                message = json.loads(line)
                reply = peer.request(message)
            except (ValueError, RuntimeError, KeyError, TypeError, OSError) as error:
                reply = dict(id=message.get('id') if isinstance(message, dict) else None,
                             ok=False, error=str(error), notifications=[])
            print(json.dumps(reply, separators=(',', ':')), flush=True)
            if len(line) > 4096 or not line.endswith(b'\n'):
                break
    finally:
        peer.close()


if __name__ == '__main__':
    main()
