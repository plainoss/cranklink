#!/usr/bin/env python3
"""Summarize host-observed DCAS block phases from an existing BLE wire trace.

Read-only: no Bluetooth connection, firmware loading, or simulator execution.
Receive timestamps describe client consumption, not radio arrival.
"""

import argparse
import json
import statistics
from pathlib import Path


def analyze(path):
    blocks = []
    current = None
    by_packet = {}
    by_index = {}
    for line in Path(path).read_text().splitlines():
        record = json.loads(line)
        value = bytes.fromhex(record.get("bytes", ""))
        characteristic = record.get("characteristic", "")[:8]
        now = record["time_ms"]
        if record.get("direction") == "tx" and characteristic == "00002afa":
            if value[:2] == b"\x80\x0b":
                current = {"start": now, "fragments": [], "packet": value[2]}
                old = by_packet.get(value[2])
                if old and not all(k in old for k in ("buffer_ack", "transfer_ack", "completion", "motor_ack")):
                    raise ValueError("packet number reused before all prior receipts")
                blocks.append(current)
                by_packet[value[2]] = current
            if len(value) == 5 and value[:2] == b"\0\x0b" and value[3] == 3:
                block = by_packet.get(value[4])
                if block:
                    block["transfer"] = now
            elif current and len(value) >= 2 and value[1] == 0x0b:
                if value[0] in (0x80, 0xa0, 0xc0, 0x60):
                    current["fragments"].append((now, value))
                    if value[0] == 0x60:
                        payload = b"".join(v[2:] for _, v in current["fragments"])
                        if len(payload) == 70:
                            by_index[int.from_bytes(payload[68:70], "little")] = current
        elif record.get("direction") == "rx" and characteristic == "00002af9":
            if len(value) >= 4 and value[0] & 0x0b == 0x0b:
                packet = (value[3] - 1) % 256 if value[2] == 0x83 else value[3]
                block = by_packet.get(packet)
                if block:
                    if value[2] == 0x82 and len(value) >= 5 and value[4] == 0:
                        block.setdefault("buffer_ack", now)
                    elif value[2] == 0x83 and len(value) >= 5 and value[4] == 0:
                        block.setdefault("transfer_ack", now)
                    elif value[2] == 0xc0:
                        block.setdefault("completion", now)
            elif len(value) >= 4 and value[:2] == b"\x88\x31":
                index = int.from_bytes(value[2:4], "little")
                if index in by_index:
                    by_index[index].setdefault("motor_ack", now)

    required = ("buffer_ack", "transfer", "transfer_ack", "completion", "motor_ack")
    complete = [b for b in blocks if all(k in b for k in required) and len(b["fragments"]) == 4]
    if not complete:
        raise ValueError("no complete indexed DCAS blocks in trace")
    for index, block in enumerate(blocks, 1):
        fragments = block["fragments"]
        if len(fragments) != 4:
            continue
        payload = b"".join(value[2:] for _, value in fragments)
        if ([value[0] for _, value in fragments] != [0x80, 0xa0, 0xc0, 0x60]
                or len(payload) != 70 or payload[1:3] != b"\x02\x00"
                or int.from_bytes(payload[68:70], "little") != index
                or sum(payload[3:67]) % 256 != payload[67]):
            raise ValueError(f"invalid fragment sequence, indexed block, or checksum at block {index}")

    def summary(values):
        ordered = sorted(values)
        return dict(count=len(values), mean_ms=round(statistics.mean(values), 3),
                    median_ms=statistics.median(values),
                    p95_ms=ordered[min(len(ordered) - 1, int(len(ordered) * .95))],
                    max_ms=max(values), total_ms=sum(values))

    phases = {
        "first_to_last_fragment": [b["fragments"][-1][0] - b["start"] for b in complete],
        "last_fragment_to_buffer_ack": [b["buffer_ack"] - b["fragments"][-1][0] for b in complete],
        "buffer_ack_to_transfer_dispatch": [b["transfer"] - b["buffer_ack"] for b in complete],
        "transfer_dispatch_to_all_receipts": [max(b[k] for k in ("transfer_ack", "completion", "motor_ack")) - b["transfer"] for b in complete],
        "block_total": [max(b[k] for k in required) - b["start"] for b in complete],
    }
    stream_ms = max(complete[-1][k] for k in required) - complete[0]["start"]
    return dict(trace=Path(path).name, observed_blocks=len(blocks), complete_indexed_blocks=len(complete),
                padded_bytes=len(complete) * 64, stream_ms=stream_ms,
                padded_bytes_per_second=round(len(complete) * 64000 / stream_ms, 3),
                phases={key: summary(values) for key, values in phases.items()},
                caveat="Host log timestamps; receive time is consumption time, not radio arrival. No ATT write-mode metadata. Ordered-batch tx records precede actual writes; inspect batch start/complete markers for host call duration.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trace", type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(args.trace), indent=2))
