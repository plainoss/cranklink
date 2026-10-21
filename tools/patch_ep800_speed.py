#!/usr/bin/env python3
"""EP800-CRG and Multi-Motor Firmware Speed Limit Patcher.

Backward-compatibility wrapper delegating to patch_motor_speed.py.
"""

from patch_motor_speed import (
    common_key,
    is_container,
    decode_container,
    encode_container,
    validate_dcas_header,
    validate_stock_image,
    encode_mov32,
    find_signatures,
    patch_binary,
    analyze_flash_layout,
    verify_flash_simulation,
    find_matching_profile,
    verify_in_emulator,
    main,
)

if __name__ == "__main__":
    main()
