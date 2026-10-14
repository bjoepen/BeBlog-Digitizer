#!/bin/bash
# Build the reviewed client source only. This script never runs the result.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
VENDOR="$REPO_ROOT/src-tauri/vendor/minolta-batch-frame"
BUILD_DIR="${BEBLOG_MINOLTA_CLIENT_BUILD_DIR:-$REPO_ROOT/src-tauri/target/minolta-hardware-test}"
OUTPUT="$BUILD_DIR/minolta-batch-frame"
SANE_INCLUDE="/opt/homebrew/include"
SANE_LIBRARY_DIR="/opt/homebrew/opt/sane-backends/lib"
SANE_LIBRARY="$SANE_LIBRARY_DIR/libsane.1.dylib"
SANE_PC="/opt/homebrew/opt/sane-backends/lib/pkgconfig/sane-backends.pc"
LIBUSB_LIBRARY="/opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib"
ORIGINAL_SHA256="35054579474360b7ce88b5288d6ec15a357a3ba4cadaacacae869c523b3741ae"

fail() {
    echo "ERROR: $*" >&2
    exit 1
}

[[ "$(uname -m)" == "arm64" ]] || fail "native arm64 macOS host required; no Rosetta build"
[[ -f "$VENDOR/minolta-batch-frame.c" ]] || fail "adapted client source missing"
[[ -f "$VENDOR/minolta_profile.c" ]] || fail "profile source missing"
[[ -f "$VENDOR/minolta-batch-frame.original.c" ]] || fail "original source snapshot missing"
ACTUAL_ORIGINAL_SHA256="$(/usr/bin/shasum -a 256 "$VENDOR/minolta-batch-frame.original.c" | /usr/bin/awk '{print $1}')"
[[ "$ACTUAL_ORIGINAL_SHA256" == "$ORIGINAL_SHA256" ]] || fail "original source snapshot hash mismatch"
[[ "$BUILD_DIR" == "$REPO_ROOT/src-tauri/target/minolta-hardware-test" || "$BUILD_DIR" == "$REPO_ROOT/src-tauri/target/minolta-d4-app-resource" || "$BUILD_DIR" == "$REPO_ROOT/src-tauri/target/minolta-d5-frame6-candidate" || "$BUILD_DIR" == "$REPO_ROOT/src-tauri/target/minolta-d5-frame6-candidate-revision-2" || "$BUILD_DIR" == "$REPO_ROOT/src-tauri/sidecars/macos-aarch64" ]] || fail "build output directory is outside the explicitly allowed locations"
[[ -f "$SANE_INCLUDE/sane/sane.h" && -f "$SANE_INCLUDE/sane/saneopts.h" ]] || fail "SANE headers missing at $SANE_INCLUDE/sane"
[[ -f "$SANE_LIBRARY" ]] || fail "SANE ARM64 library missing at $SANE_LIBRARY"
[[ -f "$SANE_PC" ]] || fail "SANE pkg-config metadata missing at $SANE_PC"
[[ -f "$LIBUSB_LIBRARY" ]] || fail "SANE's inspected libusb dependency is missing"
[[ ! -e "$OUTPUT" && ! -L "$OUTPUT" ]] || fail "refusing to overwrite existing build output: $OUTPUT"

for tool in xcrun lipo file otool codesign shasum; do
    command -v "$tool" >/dev/null 2>&1 || fail "required Apple tool unavailable: $tool"
done
CLANG="$(/usr/bin/xcrun --sdk macosx --find clang)"
SDK="$(/usr/bin/xcrun --sdk macosx --show-sdk-path)"
[[ -d "$SDK" ]] || fail "macOS SDK path is unavailable"
/usr/bin/lipo -verify_arch arm64 "$SANE_LIBRARY" >/dev/null 2>&1 || fail "libsane is not arm64"
/usr/bin/lipo -verify_arch arm64 "$LIBUSB_LIBRARY" >/dev/null 2>&1 || fail "libusb dependency is not arm64"

/bin/mkdir -p "$BUILD_DIR"
[[ ! -L "$BUILD_DIR" && -d "$BUILD_DIR" ]] || fail "build output directory must not be a symlink"
TEMP_BINARY="$(/usr/bin/mktemp "$BUILD_DIR/.minolta-batch-frame.XXXXXXXX")"
cleanup() {
    if [[ -n "${TEMP_BINARY:-}" && -e "$TEMP_BINARY" ]]; then
        /bin/rm -f "$TEMP_BINARY"
    fi
}
trap cleanup EXIT
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

echo "Building adapted client for arm64 with SDK: $SDK"
"$CLANG" \
    -arch arm64 \
    -isysroot "$SDK" \
    -I"$SANE_INCLUDE" \
    -Wall -Wextra -Werror -Wno-unused-function \
    "$VENDOR/minolta-batch-frame.c" \
    "$VENDOR/minolta_profile.c" \
    -L"$SANE_LIBRARY_DIR" \
    -Wl,-rpath,"$SANE_LIBRARY_DIR" \
    -lsane \
    -o "$TEMP_BINARY"

[[ -x "$TEMP_BINARY" ]] || fail "compiler did not produce an executable file"
/usr/bin/lipo -verify_arch arm64 "$TEMP_BINARY" >/dev/null || fail "linked output is not ARM64"
FILE_DESCRIPTION="$(/usr/bin/file "$TEMP_BINARY")"
[[ "$FILE_DESCRIPTION" == *"Mach-O 64-bit executable arm64"* ]] || fail "output is not a thin ARM64 Mach-O: $FILE_DESCRIPTION"
/usr/bin/codesign --force --sign - --identifier minolta-batch-frame "$TEMP_BINARY"
/usr/bin/codesign --verify --strict "$TEMP_BINARY"
/usr/bin/lipo -archs "$TEMP_BINARY" | /usr/bin/grep -qx arm64 || fail "output includes an unexpected architecture"

echo "Executable: $FILE_DESCRIPTION"
echo "Dynamic libraries (inspection only):"
/usr/bin/otool -L "$TEMP_BINARY"
echo "Signature details:"
/usr/bin/codesign -dvv "$TEMP_BINARY" 2>&1
echo "Final SHA-256 (after ad-hoc signing):"
/usr/bin/shasum -a 256 "$TEMP_BINARY"

# Hard-link creation is atomic and refuses an existing destination.
/bin/ln "$TEMP_BINARY" "$OUTPUT" || fail "cannot publish build output without overwriting"
/bin/rm -f "$TEMP_BINARY"
TEMP_BINARY=""
trap - EXIT HUP INT TERM
echo "Built (not executed): $OUTPUT"
