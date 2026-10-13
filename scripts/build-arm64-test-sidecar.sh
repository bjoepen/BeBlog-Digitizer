#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SOURCE="$ROOT/src-tauri/sidecars/src/test_sidecar.c"
RESOURCE_DIR="$ROOT/src-tauri/sidecars/macos-aarch64"
RESOURCE="$RESOURCE_DIR/beblog-test-sidecar"
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/beblog-test-sidecar.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT
UNSIGNED="$SCRATCH/beblog-test-sidecar-unsigned"
SIGNED="$SCRATCH/beblog-test-sidecar"

clang -arch arm64 -Wall -Wextra -Werror -Wl,-no_adhoc_codesign "$SOURCE" -o "$UNSIGNED"
mkdir -p "$ROOT/src-tauri/sidecars/test-fixtures"
install -m 755 "$UNSIGNED" "$ROOT/src-tauri/sidecars/test-fixtures/beblog-test-sidecar-unsigned"
clang -arch arm64 -Wall -Wextra -Werror "$SOURCE" -o "$SIGNED"
codesign --force --sign - "$SIGNED"
codesign --verify --strict "$SIGNED"
SIGNATURE_DETAILS="$(codesign --display --verbose=4 "$SIGNED" 2>&1)"
if ! grep -Eq 'flags=.*adhoc' <<<"$SIGNATURE_DETAILS"; then
  printf 'Expected a local ad-hoc signature on the test sidecar.\n%s\n' "$SIGNATURE_DETAILS" >&2
  exit 1
fi
ARCHS="$(lipo -archs "$SIGNED")"
if [[ "$ARCHS" != "arm64" ]]; then
  printf 'Expected thin arm64 binary, got: %s\n' "$ARCHS" >&2
  exit 1
fi
DIGEST="$(shasum -a 256 "$SIGNED" | awk '{print $1}')"
EXPECTED="$(rg -o '[0-9a-f]{64}' "$ROOT/src-tauri/src/sidecar.rs" | head -n 1)"
if [[ -z "$EXPECTED" || "$DIGEST" != "$EXPECTED" ]]; then
  printf 'Built sidecar SHA-256 %s does not match independently pinned Rust trust anchor %s\n' "$DIGEST" "${EXPECTED:-<missing>}" >&2
  printf 'Review source/build provenance and update the pin explicitly; resource was not replaced.\n' >&2
  exit 2
fi

mkdir -p "$RESOURCE_DIR"
install -m 755 "$SIGNED" "$RESOURCE"
printf 'Built and verified ARM64 test sidecar: %s\nSHA-256: %s\n' "$RESOURCE" "$DIGEST"
