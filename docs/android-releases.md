# Android releases

The Android app is published on Google Play as `app.plainoss.cranklink`. Every change to the app or the Rust core that reaches `main` is built, signed, verified and published to **internal testing**. A version reaches **production** only when a maintainer runs the production workflow for a version they installed from Play and tested.

## How it works

| Workflow | Trigger | What it does |
| --- | --- | --- |
| [Validate Android App](../.github/workflows/android.yml) | PRs, merge queue, `main` | Tests, lint, debug APK, and a release bundle signed with a disposable key. No secrets. |
| [Android Internal Release](../.github/workflows/android-release-internal.yml) | Push to `main` touching `android-app/`, `src/`, `Cargo.*` or `tools/android-release/`; manual | `build` signs and verifies the bundle with the upload key and stores it as an artifact. `publish` attests its provenance and uploads that exact bundle to the internal track. |
| [Android Production Release](../.github/workflows/android-release-production.yml) | Manual, with a version name or code | Finds the internal run that published that version, checks its bundle against Play, and sends it to production. Never rebuilds or uploads. |

The `build` job holds the upload key but no Google access; the `publish` and `promote` jobs get a short-lived Google token through GitHub OIDC but never the key. No Google service-account key exists.

`tools/android-release/build.sh` runs the shared tests, release lint and `bundleRelease`, then `verify-bundle.py` checks with checksum-pinned bundletool 1.18.3: the signature and upload certificate, application ID, version, target SDK 36, that the bundle is not debuggable, that `libcranklink_android.so` is present for exactly `arm64-v8a` and `x86_64`, 16 KB ELF alignment, and `zipalign -P 16` on a generated universal APK. It writes `release.json`, `SHA256SUMS` and `verification.txt` next to the bundle and copies the release notes from `android-app/play/release-notes.txt`. `play.py` talks to the Android Publisher API. Its tests run with `python3 -m unittest discover -s tools/android-release`.

## Versions and retries

Each internal run uses `versionCode = ANDROID_VERSION_CODE_OFFSET + run number` and `versionName = 0.1.<run number>`; local builds are 1 / 0.1.0. Don't change the offset or rename the internal workflow (which resets run numbers) without checking the highest version code already in Play.

All release workflows share one concurrency group. A build is rejected unless its commit is still the tip of `main`. Rerunning a failed run reuses the bundle and version it already built. Before uploading, `play.py internal` compares the bundle with what Play already has: an identical published bundle is a no-op, an uploaded bundle without its track update is finished without uploading again, and different bytes for the same version, a newer version in Play, or a production review in progress stop the run. It never picks a new version on its own.

Release notes for both tracks come from `android-app/play/release-notes.txt` at the built commit (at most 500 characters). Update it in the PR that changes the app, written for public users, because production reuses the notes of the version it promotes.

## Publishing to production

1. Install the version from Play internal testing. Play Console shows its name and code under **Test and release → Internal testing**. Test connecting, reading, saving and restoring on a bike, and an upgrade from the previous version.
2. Run [Android Production Release](https://github.com/plainoss/cranklink/actions/workflows/android-release-production.yml) on `main` with that version name (`0.1.7`) or code (`100007`). Running it is the confirmation that you tested it.
3. Check the run summary and Play Console's **Publishing overview**. Google's review decides when it becomes available.

The first production release has to be created in Play Console (Play rejects API releases while the app is still a draft): **Test and release → Production → Create new release**, add the existing bundle from the library, and send it for review. Then run the workflow for that version; it confirms the submission without creating another release.

Production refuses a version Play no longer lists (a newer internal build can archive it), different bytes, a downgrade, or a staged, halted or pending production release. Resolve those in Console rather than picking another version.

## Configuration

GitHub environment `android-release`, deployable from `main` only:

- Secrets: `ANDROID_UPLOAD_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`.
- Variables: `ANDROID_UPLOAD_CERTIFICATE_SHA256`, `GCP_WORKLOAD_IDENTITY_PROVIDER`, `PLAY_SERVICE_ACCOUNT`, `PLAY_RELEASE_STATUS` (`draft` until the app's first release is live, then `completed`).

GitHub environment `android-production`, deployable from `main` only, with no signing secrets: `ANDROID_UPLOAD_CERTIFICATE_SHA256`, `GCP_WORKLOAD_IDENTITY_PROVIDER`, `PLAY_SERVICE_ACCOUNT`.

Repository variables: `PLAY_INTERNAL_ENABLED` and `PLAY_PRODUCTION_ENABLED` switch publishing on (`false` pauses it; signed builds continue), and `ANDROID_VERSION_CODE_OFFSET` (default `100000`).

Configured resources:

- Play developer account `schibum` (ID 4770520382078237373); [Cranklink dashboard](https://play.google.com/console/developers/4770520382078237373/app/4974132995500525540/app-dashboard).
- Service accounts `play-cranklink@plainoss-releases.iam.gserviceaccount.com` (testing tracks) and `play-cranklink-production@plainoss-releases.iam.gserviceaccount.com` (production), each with view access and its release permission for Cranklink only.
- Workload identity providers `projects/590337448554/locations/global/workloadIdentityPools/cranklink/providers/github-actions` (accepts `android-release-internal.yml`, push or manual run, environment `android-release`) and `…/workloadIdentityPools/cranklink-production/providers/github-actions` (accepts `android-release-production.yml`, manual run only, environment `android-production`).

Google Cloud project `plainoss-releases` holds one service account and one workload identity provider per environment. Each provider accepts only this repository (by numeric ID), branch `main`, its environment, and its workflow file. In Play Console the internal account may only release to testing tracks, and the production account may only release to production, both for Cranklink only.

The upload key is generated once and kept outside the repository; back up the keystore and its passwords in a password manager, because GitHub secrets can't be read back. Google holds the app-signing key through Play App Signing. If the upload key is lost or leaked, request an upload key reset in Play Console.

## Pausing and recovery

- Set `PLAY_INTERNAL_ENABLED` or `PLAY_PRODUCTION_ENABLED` to `false` to stop new uploads or submissions. It does not withdraw anything already sent to Google; cancel running jobs separately.
- After a failed publish, read the failed step and Play Console, then rerun the same run.
- A bad release is fixed by a newer version: Android does not downgrade. For production, halt the rollout in Console if Play offers it.

The [privacy policy](android-privacy.md) and the store listing text in `android-app/play/listing/` are maintained here and entered in Play Console by hand; the workflows publish only bundles and release notes.
