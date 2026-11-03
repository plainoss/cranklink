# CLI releases

The [rolling prerelease](https://github.com/plainoss/cranklink/releases/tag/rolling) provides the latest development binaries from `main` for Linux, macOS (Apple Silicon and Intel), and Windows, with BLE support and `.sha256` checksums. Every push to `main` rebuilds all four platforms; after tests, builds, and checksum verification pass, the workflow moves the `rolling` tag to the built commit and replaces the release assets. Older runs skip publishing if `main` has advanced. These binaries may contain unreleased changes.

Pushing a `v*` tag publishes a separate versioned [release](https://github.com/plainoss/cranklink/releases). To refresh the rolling release manually, run **Build & Release CLI** with **Run workflow** on `main`. Pull requests and merge queue runs validate builds without publishing.

The prebuilt binaries are all built on Linux, and CI runs only the Linux one. The macOS binaries are not notarized: if macOS blocks the extracted binary, run `xattr -d com.apple.quarantine ./cranklink`.

The web validation and CLI release-preparation tests select the latest Node.js 22.x release, with 22.23.4 as the minimum to include the [Broccoli2Shell fix (CVE-2026-111497)](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases). This selection controls workflow shell commands; JavaScript actions use the GitHub runner's separate embedded runtime.
