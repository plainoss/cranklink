# Cranklink Android app

An Android tuning app for Shimano STEPS e-bikes, with the staged edit and backup workflow used by the [web app](../web-instrument/README.md). It connects through the bike's E-TUBE Bluetooth LE gateway. It supports drive units only; Di2 component configuration, USB/serial transport, firmware files, bootloader access, and firmware flashing are excluded.

The app requires Android 8.0 (API 26) or later and Bluetooth LE. The APK includes `arm64-v8a` for phones and `x86_64` for emulators. It has not been validated on a physical Android device connected to a bike. Build and protocol tests do not establish hardware compatibility.

## Build

Use macOS or Linux with:

- JDK 17 or 21; Java 8 cannot run the Android build. Set `JAVA_HOME` to the chosen JDK.
- Android SDK platform 35, build tools 35.0.0, platform tools, and NDK 28.2.13676358. Android Studio's SDK Manager can install these.
- Rust 1.90 or newer with `rustup`, a C/C++ build toolchain, and `rg` (ripgrep).

Set `ANDROID_HOME` to the SDK directory, or put `sdk.dir=/absolute/path/to/android/sdk` in `android-app/local.properties`. Set `ANDROID_NDK_HOME` to the NDK directory if it is outside the SDK's `ndk/` directory or several versions are installed. The native build otherwise selects an installed SDK NDK.

From the repository root:

```sh
export ANDROID_HOME=/absolute/path/to/android/sdk
export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
rustup target add aarch64-linux-android x86_64-linux-android
cd android-app
./gradlew :shared:jvmTest :androidApp:lintDebug :androidApp:assembleDebug
```

The checked-in Gradle wrapper downloads Gradle 8.11.1. Android APK builds run `scripts/build-native.sh` before packaging, so a successful APK build includes the Rust JNI library for both ABIs. The native script defaults to an optimized release library; `scripts/build-native.sh --release` is equivalent, while `scripts/build-native.sh --debug` builds debug libraries for native development.

To build only the native libraries:

```sh
./scripts/build-native.sh --release
```

It places `libcranklink_android.so` in `androidApp/src/main/jniLibs/arm64-v8a/` and `androidApp/src/main/jniLibs/x86_64/`. Generated libraries, Gradle outputs, and local SDK configuration are ignored by Git.

The debug APK is `androidApp/build/outputs/apk/debug/androidApp-debug.apk`. Install it on a connected device with:

```sh
adb install -r androidApp/build/outputs/apk/debug/androidApp-debug.apk
```

The [Android GitHub Actions workflow](https://github.com/plainoss/cranklink/actions/workflows/android.yml) builds the same debug APK and uploads it as an artifact. It is a development build signed with a debug key.

## Use

1. Close other apps connected to the bike and activate its Bluetooth connection window. See the [gateway connection instructions](../docs/bluetooth.md#connection-mode).
2. Grant the app's requested Bluetooth permissions, scan, choose the bike, and enter its six-digit E-TUBE passkey. Older Android versions also require location permission for BLE scanning.
3. Connect and let the app authenticate, identify the drive unit, and read its supported settings. Model, serial, and firmware versions appear with the tuning controls.
4. Change values locally, review the pending edits, and save. Discarding changes sends no setting writes.
5. Use backups to export or import a setting backup and restore it to the matching drive unit. Disconnect when finished so the app can release the motor's PC-link session. Master-routing sessions do not hand control back on their own, so after disconnecting (or after a failed operation) the app asks you to turn the bike off and on again to return to riding mode.

System Back asks before disconnecting a connected bike and discarding pending edits. During a save or restore it keeps the app open until the operation finishes. Cancelling a connection or read waits for the current bounded operation before disconnecting. Cleanup releases the motor and gateway session before closing Bluetooth; cancelled operations also complete this cleanup before returning.

**Demo mode** uses a separate in-memory EP800 bike. Demo reads and edits do not call JNI, scan for devices, connect to Bluetooth, or write bike settings. Its values and changes are temporary.

## Settings and backups

The native engine selects the schema from the live motor's stock identity, product attributes, and firmware version. The app includes geometry, market region, assist speed, lighting, walk assist, assist profiles, and maintenance fields when the motor supports them. Older E6000/E6001/E6100/E7000/E8000 families expose the common lamp, wheel circumference, and mounting-angle fields. E6002/E6012 also expose market selection from firmware 3.3.0.0 onward; earlier firmware exposes only the three common fields. Their markets are Europe, United States, and Korea, with no separate assist-speed setting or claimed speed cutoff. E6000/E6001/E6002 schemas limit wheel circumference to 2000–2400 mm and mounting angle to 0–45°. EP600, EP801, and EP500 use basic assist profiles; fine-tune profiles are unsupported. Unsupported or unreadable fields cannot be staged.

Each setting includes a plain-language explanation in its row and editor. Light power uses watts (1.35–27 W on EP800), speedometer calibration uses percent, and service years use the full calendar year. Values, input ranges, pending changes, and backup previews use the same units. Conversion preserves the existing protocol and backup values; see [app units](../docs/settings.md#app-descriptions-and-units).

Settings writes use the existing Rust validation, motor authorization, and BLE routing rules. The app opens a master session for STEPS tuning. Staging an assist speed outside the selected market also stages the first market that permits it (undoing the speed undoes that market change), and staging a market moves the assist speed into its range, as in the web app. Markets are named where the code is known (Europe, United States, and on EP600/EP801/EP500 Japan and US Class 3); others show their number, with the permitted speed range. The engine plans a safe write order for market and speed changes; it can reduce speed before changing region, and checks each write against the live limits. Profile torque still depends on live OEM and drivetrain limits even though the app does not expose drivetrain configuration. Modern torque changes require the verified 20–85 Nm or 20–50 Nm level tables; other factory ranges are refused. Only choose the market region that applies where the bike is ridden; changing settings can affect compliance with local e-bike rules.

Each setting write durably saves its original value before sending a SET and verifies the result by reading it back. A multi-setting save is sequential: if a later write fails, earlier successful writes remain applied. There is no automatic rollback. The app stops after an uncertain outcome, clears writable live state, and requires reconnecting and reading the bike before another save. A successful readback confirms the current live value; persistence after a power cycle needs another read after restarting the bike.

Backups are ordinary `cranklink-setting-backup-v1` JSON files kept in the app's private storage, alongside transaction journals. Each backup covers **one setting** and carries its motor model and serial. Importing a file does not write the bike. Restore checks its model, serial, setting name, and value against the current motor, saves a new backup of the replaced value, and verifies the restored value. Backups without matching model metadata are rejected. Export important backups before clearing app data or uninstalling.

The passkey stays in the active connection's memory and is not included in settings backups or traces.

## Implementation and tests

- `shared/` is a Kotlin Multiplatform module targeting Android and JVM. It holds the identity and schema gates, value validation, staged edits, serial-bound save/restore requests, and the coroutine session state machine. Its common tests run on JVM without a bike.
- `androidApp/` provides Jetpack Compose controls, Android BLE scanning, serialized GATT callbacks, permissions, and backup import/export. All JNI calls run on one worker because native sessions retain thread-local connection state.
- `native/` builds `libcranklink_android.so`. The small JNI boundary adapts Android GATT operations into the existing Rust BLE engine and limits requests to STEPS settings operations. It exposes no raw command or firmware operation.

Run the focused tests from `android-app/`:

```sh
cargo test --locked --manifest-path native/Cargo.toml
./gradlew :shared:jvmTest :androidApp:lintDebug
```

The shared tests cover staging and discard behavior, model/schema admission, unavailable and read-only fields, scaled numeric limits, region ordering, identity changes, incomplete readbacks, partial-save recovery, restore guards, and concurrent operations. Native tests cover request and backup guards and the restricted settings surface. The root Rust suite also checks BLE authentication, routing, protocol receipts, and model-specific settings encoding.

To include Android native tests, shared tests, lint, and APK assembly in the repository's full verification, run from the repository root with the Android prerequisites configured:

```sh
CRANKLINK_VERIFY_ANDROID=1 ./tools/verify.sh
```

With an Android emulator or test device running, execute the device smoke tests from `android-app/`:

```sh
./gradlew :androidApp:connectedDebugAndroidTest
```

These exercise the Compose demo staging/review/save flow without Bluetooth permission, check watt inputs and percentage display through review and saved readback, and load the packaged JNI library to check that firmware and component requests are rejected. They do not connect to a real BLE gateway or validate bike writes.
