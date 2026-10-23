#!/usr/bin/env bash
set -euo pipefail

APP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE=release
if [[ "${1:-}" == "--debug" ]]; then
  PROFILE=debug
  shift
elif [[ "${1:-}" == "--release" ]]; then
  shift
fi
if [[ "$#" -ne 0 ]]; then
  echo "Usage: $0 [--release|--debug]" >&2
  exit 2
fi

SDK_ROOT="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
if [[ -z "$SDK_ROOT" && -f "$APP_ROOT/local.properties" ]]; then
  SDK_ROOT="$(sed -n 's/^sdk\.dir=//p' "$APP_ROOT/local.properties" | head -n 1)"
fi
if [[ -z "$SDK_ROOT" ]]; then
  case "$(uname -s)" in
    Darwin) SDK_ROOT="$HOME/Library/Android/sdk" ;;
    Linux) SDK_ROOT="$HOME/Android/Sdk" ;;
  esac
fi
NDK_ROOT="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
if [[ -z "$NDK_ROOT" && -d "$SDK_ROOT/ndk" ]]; then
  NDK_ROOT="$(find "$SDK_ROOT/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
fi
if [[ ! -d "$NDK_ROOT/toolchains/llvm/prebuilt" ]]; then
  echo "Android NDK missing. Install an NDK with Android Studio or set ANDROID_NDK_HOME." >&2
  exit 1
fi
case "$(uname -s)" in
  Darwin) HOST_TAG=darwin-x86_64 ;;
  Linux) HOST_TAG=linux-x86_64 ;;
  *) echo "Build native Android libraries from macOS or Linux." >&2; exit 1 ;;
esac
TOOLCHAIN="$NDK_ROOT/toolchains/llvm/prebuilt/$HOST_TAG/bin"
if [[ ! -x "$TOOLCHAIN/aarch64-linux-android26-clang" ]]; then
  echo "Android NDK toolchain unavailable at $TOOLCHAIN" >&2
  exit 1
fi
export PATH="$TOOLCHAIN:$PATH"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TOOLCHAIN/aarch64-linux-android26-clang"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$TOOLCHAIN/x86_64-linux-android26-clang"
export CC_aarch64_linux_android="$TOOLCHAIN/aarch64-linux-android26-clang"
export CC_x86_64_linux_android="$TOOLCHAIN/x86_64-linux-android26-clang"
export AR_aarch64_linux_android="$TOOLCHAIN/llvm-ar"
export AR_x86_64_linux_android="$TOOLCHAIN/llvm-ar"

for TARGET in aarch64-linux-android x86_64-linux-android; do
  if ! rustup target list --installed | rg -qx "$TARGET"; then
    rustup target add "$TARGET"
  fi
  CARGO_ARGUMENTS=(build --locked --manifest-path "$APP_ROOT/native/Cargo.toml" --target "$TARGET")
  if [[ "$PROFILE" == release ]]; then
    CARGO_ARGUMENTS+=(--release)
  fi
  cargo "${CARGO_ARGUMENTS[@]}"
  case "$TARGET" in
    aarch64-linux-android) ABI=arm64-v8a ;;
    x86_64-linux-android) ABI=x86_64 ;;
  esac
  OUTPUT="$APP_ROOT/androidApp/src/main/jniLibs/$ABI"
  mkdir -p "$OUTPUT"
  cp "$APP_ROOT/native/target/$TARGET/$PROFILE/libcranklink_android.so" "$OUTPUT/libcranklink_android.so"
done
