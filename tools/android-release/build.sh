#!/usr/bin/env bash
# Build, sign and verify the Cranklink release bundle into an output directory.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
: "${JAVA_HOME:?Set JAVA_HOME to JDK 17 or 21}"
: "${ANDROID_HOME:?Set ANDROID_HOME}"
: "${ANDROID_VERSION_CODE:?Set ANDROID_VERSION_CODE}"
: "${ANDROID_VERSION_NAME:?Set ANDROID_VERSION_NAME}"
: "${ANDROID_UPLOAD_CERTIFICATE_SHA256:?Set the expected upload certificate SHA-256}"
output="${1:?Usage: tools/android-release/build.sh /absolute/output/directory}"
mkdir -p "$output"
output="$(cd "$output" && pwd)"
tool="$(mktemp -d)/bundletool.jar"
trap 'rm -rf "$(dirname "$tool")"' EXIT
curl --fail --location --silent --show-error --retry 3 \
  https://github.com/google/bundletool/releases/download/1.18.3/bundletool-all-1.18.3.jar -o "$tool"
echo "a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29  $tool" | shasum -a 256 --check --quiet
(
  cd "$root/android-app"
  ./gradlew --no-daemon --no-configuration-cache --console=plain \
    :shared:jvmTest :androidApp:lintRelease :androidApp:bundleRelease
)
cp "$root/android-app/androidApp/build/outputs/bundle/release/androidApp-release.aab" "$output/"
python3 "$root/tools/android-release/verify-bundle.py" \
  --bundle "$output/androidApp-release.aab" --bundletool "$tool" \
  --certificate-sha256 "$ANDROID_UPLOAD_CERTIFICATE_SHA256" \
  --version-code "$ANDROID_VERSION_CODE" --version-name "$ANDROID_VERSION_NAME" \
  --commit "$(git -C "$root" rev-parse HEAD)" --output "$output"
cp "$root/android-app/play/release-notes.txt" "$output/release-notes.txt"
