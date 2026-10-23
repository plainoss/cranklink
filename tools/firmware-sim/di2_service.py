#!/usr/bin/env python3
"""Run pinned Di2 firmware PC-link service experiments entirely offline."""
import argparse
import json
from pathlib import Path
from fw_sim.di2_service import Di2Service, PROFILES


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--images', type=Path, default=Path(__file__).parent / '.cache/di2')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    rows = []
    for name in PROFILES:
        peer = Di2Service(args.images / name)
        start = peer.link(1, 19)
        # A lost read reply and radio close do not execute any firmware stop.
        stranded = dict(connected=peer.connected, owner=peer.owner)
        stop = peer.link(0, 19)
        rows.append(dict(filename=name, sha256=peer.assessment['sha256'],
                         start=start, read_timeout_without_stop=stranded, stop=stop))
    report = dict(scope='PC-link service handlers; modeled receive/send boundaries',
                  full_topology_validated=False, cases=rows)
    args.output.write_text(json.dumps(report, indent=2) + '\n')
    for row in rows:
        print(row['filename'], 'start returned:', row['start']['returned'],
              'remains active without stop:', row['read_timeout_without_stop']['connected'],
              'stop receipt:', row['stop']['receipt_boundary'],
              'stop error:', row['stop']['error'])


if __name__ == '__main__':
    main()
