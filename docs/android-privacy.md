# Cranklink Android privacy policy

Effective 30 October 2026. This policy covers the Cranklink Android app (`app.plainoss.cranklink`), published by PlainOSS.

## What the app collects

Nothing. The app has no account, no analytics, no advertising and no crash reporting, and it does not request the Internet permission, so it cannot send data anywhere.

## What stays on your phone

- **Bluetooth:** the app scans for and connects to your bike's E-TUBE Bluetooth gateway to read and change drive unit settings. On Android 11 and older, Android requires the location permission for Bluetooth scanning; the app does not read your location.
- **Passkey:** the six-digit E-TUBE passkey you enter is kept in memory for the active connection only and is never stored.
- **Settings backups and transaction journals:** before each write the app saves the previous value, with the drive unit's model and serial number, in the app's private storage. They are removed when you clear the app's data or uninstall it. Android cloud backup is disabled for the app.
- **Exported backups:** when you export a backup, it is written where you choose through Android's file picker. Where it goes from there is up to you.

## Contact

Questions about this policy: open an issue at https://github.com/plainoss/cranklink/issues.
