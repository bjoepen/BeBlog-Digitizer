#!/bin/bash
# Standalone, manual-only D3-B5b test entrypoint. Never called by Tauri or CI.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd -P)"
source "$REPO_ROOT/tools/process-timeout.sh"
CLIENT="$REPO_ROOT/src-tauri/target/minolta-hardware-test/minolta-batch-frame"
TRUST_FILE="$REPO_ROOT/tools/minolta-hardware-test.sha256"
FRAME=1
DPI=2820
HOLDER_TIMEOUT=120
POLL_INTERVAL=2
TOTAL_TIMEOUT_SECONDS=1200
TERMINATION_GRACE_SECONDS=15

usage() {
    echo "Usage: $0 EXACT_SANE_DEVICE_IDENTIFIER" >&2
    echo "This standalone command performs one explicitly confirmed hardware scan." >&2
}

if [[ $# -ne 1 || -z "$1" || "$1" == -* || "$1" == *$'\n'* || "$1" == *$'\r'* ]]; then
    usage
    exit 2
fi
DEVICE="$1"

verify_client() {
    if [[ ! -f "$TRUST_FILE" || -L "$TRUST_FILE" ]]; then
        echo "Missing independent client SHA-256 approval file: $TRUST_FILE" >&2
        return 1
    fi
    EXPECTED_HASH="$(/usr/bin/awk 'NR == 1 { if (NF != 1) exit 1; value = $1 } NR > 1 { exit 1 } END { if (NR != 1) exit 1; print value }' "$TRUST_FILE" | /usr/bin/tr '[:upper:]' '[:lower:]')" || {
        echo "Trust file must contain one SHA-256 token on one line" >&2
        return 1
    }
    if [[ ! "$EXPECTED_HASH" =~ ^[[:xdigit:]]{64}$ ]]; then
        echo "Trust file must contain exactly one 64-digit SHA-256 value" >&2
        return 1
    fi
    if [[ ! -f "$CLIENT" || -L "$CLIENT" || ! -x "$CLIENT" ]]; then
        echo "Approved ARM64 client is missing, non-regular, symlinked, or not executable: $CLIENT" >&2
        return 1
    fi
    ARCHS="$(/usr/bin/lipo -archs "$CLIENT")"
    if [[ "$ARCHS" != "arm64" ]]; then
        echo "Client must be thin arm64 only; found: $ARCHS" >&2
        return 1
    fi
    ACTUAL_HASH="$(/usr/bin/shasum -a 256 "$CLIENT" | /usr/bin/awk '{print $1}')"
    if [[ "$ACTUAL_HASH" != "$EXPECTED_HASH" ]]; then
        echo "Client SHA-256 does not match the independently approved value" >&2
        return 1
    fi
}

verify_client || exit 2

echo "Hardware test proposal (no device discovery is performed):"
echo "  Client: $CLIENT"
echo "  SHA-256: $ACTUAL_HASH"
echo "  Device: $DEVICE"
echo "  Profile: frame=$FRAME, RGB8, requested DPI=$DPI"
echo "  Holder poll only: timeout=${HOLDER_TIMEOUT}s, interval=${POLL_INTERVAL}s"
echo "  Full-process timeout: ${TOTAL_TIMEOUT_SECONDS}s; SIGTERM cleanup grace: ${TERMINATION_GRACE_SECONDS}s"
echo "  Client will be started exactly once; no retries or resume."
read -r -p "Type the exact device identifier to confirm: " DEVICE_CONFIRM
if [[ "$DEVICE_CONFIRM" != "$DEVICE" ]]; then
    echo "Device confirmation did not match; no output directory or process created." >&2
    exit 1
fi
read -r -p "Type START ONE FRAME 1 AT 2820 DPI to authorize this single run: " START_CONFIRM
if [[ "$START_CONFIRM" != "START ONE FRAME 1 AT 2820 DPI" ]]; then
    echo "Start authorization not confirmed; no output directory or process created." >&2
    exit 1
fi

# Recheck after the interactive pause. This narrows but does not eliminate the
# path-based check-to-exec race; that remains an explicit local-test limitation.
verify_client || exit 2

OUTPUT_ROOT="$HOME/Documents/BeBlog Digitizer Hardware Tests"
/bin/mkdir -p "$OUTPUT_ROOT"
OUTPUT_DIR="$(/usr/bin/mktemp -d "$OUTPUT_ROOT/frame1-2820-XXXXXXXX")"
PREFIX="$OUTPUT_DIR/capture"
printf 'output_directory=%s\nclient_sha256=%s\ndevice=%s\nframe=%s\nrequested_dpi=%s\ncolor=RGB8\nholder_poll_timeout_seconds=%s\npoll_interval_seconds=%s\nprocess_total_timeout_seconds=%s\ntermination_grace_seconds=%s\n' \
    "$OUTPUT_DIR" "$ACTUAL_HASH" "$DEVICE" "$FRAME" "$DPI" "$HOLDER_TIMEOUT" "$POLL_INTERVAL" \
    "$TOTAL_TIMEOUT_SECONDS" "$TERMINATION_GRACE_SECONDS" > "$OUTPUT_DIR/test-plan.txt"

echo "Starting one Minolta client process once; total timeout=${TOTAL_TIMEOUT_SECONDS}s, TERM grace=${TERMINATION_GRACE_SECONDS}s."
echo "The client -t=$HOLDER_TIMEOUT remains only the holder-detection timeout."
beblog_run_with_timeout "$TOTAL_TIMEOUT_SECONDS" "$TERMINATION_GRACE_SECONDS" \
    "$OUTPUT_DIR/stdout.txt" "$OUTPUT_DIR/stderr.txt" "$CLIENT" \
    -d "$DEVICE" -o "$PREFIX" -f "$FRAME" -r "$DPI" -t "$HOLDER_TIMEOUT" -i "$POLL_INTERVAL"
CLIENT_STATUS="$BEBLOG_PROCESS_EXIT"

{
    printf 'termination_reason=%s\nclient_wait_status=%s\nsigterm_sent=%s\nsigkill_sent=%s\n' \
        "$BEBLOG_PROCESS_CAUSE" "$CLIENT_STATUS" "$BEBLOG_PROCESS_TERM_SENT" "$BEBLOG_PROCESS_KILL_SENT"
    if [[ "$BEBLOG_PROCESS_CAUSE" == timeout || "$BEBLOG_PROCESS_CAUSE" == cancelled_* ||
          "$BEBLOG_PROCESS_CAUSE" == start_failed || "$CLIENT_STATUS" -ne 0 ]]; then
        echo 'capture_status=INCOMPLETE'
    else
        echo 'capture_status=PENDING_OFFLINE_ARTIFACT_VALIDATION'
    fi
} >> "$OUTPUT_DIR/test-plan.txt"
beblog_record_artifacts "$PREFIX" "$OUTPUT_DIR/test-plan.txt"
if [[ "$BEBLOG_MISSING_ARTIFACT_COUNT" -gt 0 ]]; then
    printf 'capture_status=INCOMPLETE_MISSING_ARTIFACTS\nmissing_artifact_count=%s\n' \
        "$BEBLOG_MISSING_ARTIFACT_COUNT" >> "$OUTPUT_DIR/test-plan.txt"
fi

echo "Process wait status: $CLIENT_STATUS; termination reason: $BEBLOG_PROCESS_CAUSE; results: $OUTPUT_DIR"
echo "No retry is performed. All artifacts require offline validation; this launcher never marks a Production or completed capture."
case "$BEBLOG_PROCESS_CAUSE" in
    timeout) exit 124 ;;
    cancelled_sigint) exit 130 ;;
    cancelled_sigterm) exit 143 ;;
    start_failed) exit 127 ;;
esac
if [[ "$CLIENT_STATUS" -ne 0 || "$BEBLOG_MISSING_ARTIFACT_COUNT" -gt 0 ]]; then
    exit 1
fi
exit 0
