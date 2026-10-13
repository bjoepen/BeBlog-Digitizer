#!/bin/bash
set -euo pipefail

TEST_DIR="$(cd "$(dirname "$0")" && pwd -P)"
REPO_ROOT="$(cd "$TEST_DIR/../.." && pwd -P)"
source "$REPO_ROOT/tools/process-timeout.sh"
TEMP_ROOT="$(/usr/bin/mktemp -d "${TMPDIR:-/tmp}/beblog-timeout-tests.XXXXXXXX")"
cleanup() {
    /bin/rm -f "$TEMP_ROOT"/*
    /bin/rmdir "$TEMP_ROOT" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM

PROBE="$TEMP_ROOT/timeout-probe"
/usr/bin/clang -Wall -Wextra -Werror "$TEST_DIR/timeout-probe.c" -o "$PROBE"

assert_eq() {
    [[ "$1" == "$2" ]] || {
        echo "FAIL: expected '$2', got '$1'" >&2
        exit 1
    }
}

marker_lines() {
    /usr/bin/wc -l < "$1" | /usr/bin/tr -d '[:space:]'
}

beblog_run_with_timeout 4 2 "$TEMP_ROOT/success.out" "$TEMP_ROOT/success.err" "$PROBE" success "$TEMP_ROOT/success.marker"
assert_eq "$BEBLOG_PROCESS_CAUSE" exited
assert_eq "$BEBLOG_PROCESS_EXIT" 0
assert_eq "$(marker_lines "$TEMP_ROOT/success.marker")" 1

beblog_run_with_timeout 4 2 "$TEMP_ROOT/nonzero.out" "$TEMP_ROOT/nonzero.err" "$PROBE" exit7 "$TEMP_ROOT/nonzero.marker"
assert_eq "$BEBLOG_PROCESS_CAUSE" exited
assert_eq "$BEBLOG_PROCESS_EXIT" 7
assert_eq "$(marker_lines "$TEMP_ROOT/nonzero.marker")" 1

beblog_run_with_timeout 1 3 "$TEMP_ROOT/term.out" "$TEMP_ROOT/term.err" "$PROBE" cooperative "$TEMP_ROOT/term.marker"
assert_eq "$BEBLOG_PROCESS_CAUSE" timeout
assert_eq "$BEBLOG_PROCESS_TERM_SENT" yes
assert_eq "$BEBLOG_PROCESS_KILL_SENT" no
assert_eq "$BEBLOG_PROCESS_EXIT" 0
assert_eq "$(marker_lines "$TEMP_ROOT/term.marker")" 1

beblog_run_with_timeout 1 1 "$TEMP_ROOT/kill.out" "$TEMP_ROOT/kill.err" "$PROBE" ignore-term "$TEMP_ROOT/kill.marker"
assert_eq "$BEBLOG_PROCESS_CAUSE" timeout
assert_eq "$BEBLOG_PROCESS_TERM_SENT" yes
assert_eq "$BEBLOG_PROCESS_KILL_SENT" yes
assert_eq "$BEBLOG_PROCESS_EXIT" 137
assert_eq "$(marker_lines "$TEMP_ROOT/kill.marker")" 1

(
    beblog_run_with_timeout 20 2 "$TEMP_ROOT/cancel.out" "$TEMP_ROOT/cancel.err" "$PROBE" cooperative "$TEMP_ROOT/cancel.marker"
    printf '%s\n%s\n%s\n' "$BEBLOG_PROCESS_CAUSE" "$BEBLOG_PROCESS_EXIT" "$BEBLOG_PROCESS_TERM_SENT" > "$TEMP_ROOT/cancel.result"
) &
SUPERVISOR_PID=$!
for _ in 1 2 3 4 5; do
    [[ -f "$TEMP_ROOT/cancel.marker" ]] && break
    /bin/sleep 1
done
[[ -f "$TEMP_ROOT/cancel.marker" ]]
/bin/kill -TERM "$SUPERVISOR_PID"
wait "$SUPERVISOR_PID"
assert_eq "$(/usr/bin/sed -n '1p' "$TEMP_ROOT/cancel.result")" cancelled_sigterm
assert_eq "$(/usr/bin/sed -n '2p' "$TEMP_ROOT/cancel.result")" 0
assert_eq "$(/usr/bin/sed -n '3p' "$TEMP_ROOT/cancel.result")" yes
assert_eq "$(marker_lines "$TEMP_ROOT/cancel.marker")" 1

beblog_run_with_timeout 1 1 "$TEMP_ROOT/missing.out" "$TEMP_ROOT/missing.err" "$TEMP_ROOT/absent" arg "$TEMP_ROOT/missing.marker"
assert_eq "$BEBLOG_PROCESS_CAUSE" start_failed
assert_eq "$BEBLOG_PROCESS_EXIT" 127
[[ ! -e "$TEMP_ROOT/missing.marker" ]]

# Artifact reporting uses only fixed suffixes and never promotes presence to validity.
beblog_record_artifacts "$TEMP_ROOT/capture" "$TEMP_ROOT/artifacts.result"
assert_eq "$BEBLOG_MISSING_ARTIFACT_COUNT" 4
printf 'synthetic-incomplete-raw' > "$TEMP_ROOT/capture.raw"
/bin/ln -s "$TEMP_ROOT/capture.raw" "$TEMP_ROOT/capture.tiff"
beblog_record_artifacts "$TEMP_ROOT/capture" "$TEMP_ROOT/artifacts.symlink-result"
assert_eq "$BEBLOG_MISSING_ARTIFACT_COUNT" 3
[[ "$(/usr/bin/grep -c 'artifact_validation=not_performed' "$TEMP_ROOT/artifacts.symlink-result")" -eq 1 ]]

echo "PASS: success, nonzero exit, timeout/SIGTERM, SIGKILL escalation, cancellation, wait/reap, single start, start failure, incomplete/symlink artifacts"
