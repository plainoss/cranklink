# Native simulator feature checks

On 17 October 2026, `verify_features.py` ran the actual native `target/debug/cranklink` binary against independent stock-firmware endpoints. The binary was built from the worktree based on simulator commit `290bfc9`; these results describe that revision, before integrating PR #11's broader family/session changes.

Twelve feature/guard cases passed on each pair:

| Gateway | Motor | Result | Full snapshot |
| --- | --- | --- | --- |
| SC-EM800 4.6.0 | DU-EP800 4.2.0 | 12/12 | 73 readable settings; maintenance date reported unavailable |
| SC-E6100 4.1.0 | DU-EP600 4.6.0 | 12/12 | 56 readable settings; maintenance date reported unavailable |

Five additional EP800 checks passed after strengthening output assertions and proving that offline/preflight operations do not attempt a connection. Servers ran independently with at most four concurrent cases. Product delays and timeout behavior were retained. This supplements `verify_native.py`'s probe/lamp/model-guard/drop-reply checks.

## Cases and interpretation

| Case | Observed result |
| --- | --- |
| `units` | JSON inventory identifies the motor at slot 13. |
| `settings read destination` | Returns a parsed value and nonempty raw response. |
| Full `settings read --output` | Exported JSON matches stdout and includes lamp/destination values. Unavailable maintenance date remains explicit. |
| `settings schema lamp` | Returns the lamp schema. It also succeeds with a nonexistent simulator socket, proving the offline path. |
| Read-only batch plan | Two ordered read operations complete and create a local journal; no setting mutation is requested. This tests a plan containing only reads, without the global `--read-only` option, which conservatively rejects batch commands. |
| Lamp `--dry-run --no-auth` | Reads the current value, validates the planned value and produces no pre-write backup. |
| Incorrect expected serial | Rejects the mismatch. Serial data is synthetic zero-filled NVM, not a real device identifier. |
| Missing `--component DU-EP801` | Rejects selection with no matching slots. |
| `--read-only settings set` | Rejects mutation before attempting a connection, verified using a nonexistent socket. |
| OEM destination write with `--no-auth` | Rejects required drive-unit authorization and creates no pre-write backup. |
| Attempted lamp write at the endpoint | Creates the client's durable backup, then the peer explicitly rejects the write as read-only. The client reports failure and a closed-session/cleanup error. This is an intentional unsupported simulator boundary; it does not establish a motor write or persistent-state outcome. |
| Malformed restore file | Rejects invalid JSON before attempting a connection, verified using a nonexistent socket. |

There were no unexplained failures in these cases. Initial harness mismatches were corrected to match the CLI's actual JSON envelope and error text; successful commands and expected rejections were rerun. The maintenance-date error is consistent with uninitialized calendar data in synthetic NVM. It is retained in the snapshot rather than replaced with an invented valid date.

These checks validate the native CLI's read/export/planning and rejection paths against stock gateway/motor instructions. Adapter behavior, NVM contents and the absent M application version response are modeled. Capability flags in exported schema describe Cranklink's model support; they do not turn a simulator run into hardware validation. These tests do not cover actual setting writes, persistent storage, USB hardware, browser retained connections, Bluetooth radio delivery or repeated browser saves from [PR #11](simulator-pr-review.md#would-it-have-caught-pr-11s-bluetooth-issue).

## Reproduce

Use a local manifest mapping executable profile IDs to decoded firmware paths; no firmware images are committed:

```sh
python tools/firmware-sim/verify_features.py \
  --binary target/debug/cranklink \
  --fixtures /absolute/path/fixtures.local.json \
  --output /tmp/cranklink-feature-results.json

python tools/firmware-sim/verify_features.py \
  --binary target/debug/cranklink \
  --fixtures /absolute/path/fixtures.local.json \
  --gateway sc-e6100-4.1.0 --motor ep600-4.6.0 \
  --output /tmp/cranklink-feature-ep600.json
```

Run with the Python environment containing `tools/firmware-sim/requirements.txt`. Reports include raw CLI output, temporary paths and synthetic parameter data and should remain local. This document records only aggregate results and public model/version identifiers; no firmware, authentication proofs or raw transport traces are included.
