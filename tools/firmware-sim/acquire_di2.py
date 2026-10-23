#!/usr/bin/env python3
"""Acquire hash-pinned WU111-family/BT-DN110 research inputs, without hardware."""
import argparse
import json
from pathlib import Path
from fw_sim.di2_images import acquire


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path,
                        default=Path(__file__).resolve().parent / '.cache' / 'di2')
    parser.add_argument('--offline', action='store_true', help='validate cached files without network access')
    args = parser.parse_args()
    report = acquire(args.output, offline=args.offline)
    print(json.dumps(dict(images=len(report['images']), executable_profiles_added=0,
                          topology_validated=False, report=str(args.output / 'di2-acquisition.json'))))


if __name__ == '__main__':
    main()
