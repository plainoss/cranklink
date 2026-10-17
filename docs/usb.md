# SM-PCE02 USB connection

On Android, select **USB** in the browser connection options. WebUSB communicates directly with the adapter; the SM-PCE02 need not appear in the browser's serial chooser. Use `--port usb` for native direct USB access.

The supported adapter is `1E44:7151`, with interface 0, alternate 0 and 64-byte bulk endpoints. Device revision `0x0A00` uses the existing 2 Mbaud divisor. Android WebUSB verification on 17 October 2026 identified a DU-EP800-CRG and read 74 parameters with this revision.

## Charging during initialization

The SM-PCE02 has an internal supercapacitor used to supply the E-TUBE bus. After reset and power-limit unlock (`1A/01`), the power query (`1A/02`, reply `3A/02`) returns:

| Raw state | Initialization behavior |
| --- | --- |
| `01` | External power is present; preserve it. |
| `02` | Capacitor is ready; enable adapter supply with `1A/03`. |
| `03`, `04` | Wait for charging, then recheck readiness. |
| Other | Stop with an error. |

Charging happens automatically. Cranklink waits three seconds between capacitor-level queries (`1C`, reply `3C`, little-endian 16-bit value), and queries the power state once the level reaches **2703**. The adapter must report `01` or `02` before initialization continues. Cranklink waits up to 90 seconds, without resetting the adapter or enabling discharge during charging. The browser displays progress; native commands write it to stderr so JSON stdout remains parseable.

This fixes an immediate failure previously observed on both desktop and Android with raw state `04`. If charging times out, keep USB connected and check its power source before retrying. The timeout reports the last queried raw power state. Malformed replies and transport errors still stop initialization.

Protocol regression checks cover a low capacitor level, a charging state that persists after the voltage threshold, transition to adapter supply, transition to external power, and a truncated voltage reply. The successful Android connection had already reached a ready power state; it does not establish the duration of charging from an empty capacitor.
