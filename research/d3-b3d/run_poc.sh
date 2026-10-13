#!/bin/bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
LAB="$(mktemp -d "${TMPDIR:-/tmp}/beblog-d3-b3d.XXXXXX")"
printf 'Research test directory: %s\n' "$LAB"

clang -Wall -Wextra -Werror "$ROOT/identity_poc.c" -framework Security -framework CoreFoundation -o "$LAB/identity-poc"
clang -Wall -Wextra -Werror -DCHILD_MARKER_TEXT='"child-A-ran"' "$ROOT/harmless_child.c" -o "$LAB/child-A"
clang -Wall -Wextra -Werror -DCHILD_MARKER_TEXT='"child-B-ran"' "$ROOT/harmless_child.c" -o "$LAB/child-B"
clang -Wall -Wextra -Werror -DCHILD_MARKER_TEXT='"unsigned-child-ran"' "$ROOT/harmless_child.c" -o "$LAB/child-unsigned"
codesign --force --sign - "$LAB/child-A"
codesign --force --sign - "$LAB/child-B"

run_signed_case() {
    local scenario="$1"
    local target="$LAB/$scenario-target"
    local replacement="$LAB/$scenario-replacement"
    local marker="$LAB/$scenario-marker"
    cp "$LAB/child-A" "$target"
    cp "$LAB/child-B" "$replacement"
    "$LAB/identity-poc" "$scenario" "$target" "$replacement" "$marker"
}

run_signed_case success
run_signed_case wrong-hash
run_signed_case swap-before-spawn
run_signed_case swap-after-spawn
run_signed_case swap-after-validity
run_signed_case remove-after-validity
run_signed_case tampered
run_signed_case lookup-failure
run_signed_case unexpected-end
run_signed_case abort-before-continue
run_signed_case resume-failure
run_signed_case kill-failure
run_signed_case reap-failure
run_signed_case remove-before-spawn
"$LAB/identity-poc" spawn-failure "$LAB/does-not-exist" "$LAB/no-replacement" "$LAB/spawn-failure-marker"

UNSIGNED_TARGET="$LAB/unsigned-target"
cp "$LAB/child-unsigned" "$UNSIGNED_TARGET"
"$LAB/identity-poc" unsigned "$UNSIGNED_TARGET" "$LAB/no-replacement" "$LAB/unsigned-marker"

printf '\nAll expected pass/fail assertions completed. Test artifacts retained at %s\n' "$LAB"
