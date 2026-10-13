#!/bin/bash
# Source-only direct-child timeout supervisor for standalone manual test tools.
# Does not use a shell command string, PATH lookup, or an external timeout package.

_beblog_process_running() {
    local pid="$1"
    local stat
    /bin/kill -0 "$pid" 2>/dev/null || return 1
    stat="$(/bin/ps -p "$pid" -o stat= 2>/dev/null | /usr/bin/awk '{print $1}' || true)"
    if [[ -z "$stat" ]]; then
        # If kill -0 still succeeds but ps could not report state, fail closed:
        # assume the unreaped direct child may still be running.
        return 0
    fi
    [[ "$stat" != Z* ]]
}

_beblog_timeout_signal() {
    _BEBLOG_SIGNAL_REASON="$1"
}

# Result variables set on every return:
#   BEBLOG_PROCESS_CAUSE: exited | timeout | cancelled_sigint | cancelled_sigterm | start_failed
#   BEBLOG_PROCESS_EXIT: actual status returned by wait (127 if not started)
#   BEBLOG_PROCESS_TERM_SENT / BEBLOG_PROCESS_KILL_SENT: yes | no
# Arguments: total-seconds grace-seconds stdout-path stderr-path executable arg...
beblog_run_with_timeout() {
    local timeout_seconds="$1" grace_seconds="$2" stdout_path="$3" stderr_path="$4"
    shift 4
    BEBLOG_PROCESS_CAUSE=start_failed
    BEBLOG_PROCESS_EXIT=127
    BEBLOG_PROCESS_TERM_SENT=no
    BEBLOG_PROCESS_KILL_SENT=no
    _BEBLOG_SIGNAL_REASON=""

    if [[ ! "$timeout_seconds" =~ ^[0-9]+$ || ! "$grace_seconds" =~ ^[0-9]+$ ]] ||
       (( timeout_seconds < 1 || grace_seconds < 1 )) || (( $# == 0 )) ||
       [[ ! -f "$1" || ! -x "$1" ]]; then
        return 0
    fi

    trap '_beblog_timeout_signal sigint' INT
    trap '_beblog_timeout_signal sigterm' TERM
    "$@" > "$stdout_path" 2> "$stderr_path" &
    local child_pid=$!
    local started_at=$SECONDS

    BEBLOG_PROCESS_CAUSE=exited
    while _beblog_process_running "$child_pid"; do
        if [[ "$_BEBLOG_SIGNAL_REASON" == sigint ]]; then
            BEBLOG_PROCESS_CAUSE=cancelled_sigint
            break
        elif [[ "$_BEBLOG_SIGNAL_REASON" == sigterm ]]; then
            BEBLOG_PROCESS_CAUSE=cancelled_sigterm
            break
        fi
        if (( SECONDS - started_at >= timeout_seconds )); then
            BEBLOG_PROCESS_CAUSE=timeout
            break
        fi
        /bin/sleep 1 || true
    done

    if [[ "$BEBLOG_PROCESS_CAUSE" != exited ]]; then
        if /bin/kill -TERM "$child_pid" 2>/dev/null; then
            BEBLOG_PROCESS_TERM_SENT=yes
        fi
        local grace_started_at=$SECONDS
        while _beblog_process_running "$child_pid" &&
              (( SECONDS - grace_started_at < grace_seconds )); do
            /bin/sleep 1 || true
        done
        if _beblog_process_running "$child_pid"; then
            if /bin/kill -KILL "$child_pid" 2>/dev/null; then
                BEBLOG_PROCESS_KILL_SENT=yes
            fi
        fi
    fi

    local child_status
    if wait "$child_pid"; then
        child_status=0
    else
        child_status=$?
    fi
    BEBLOG_PROCESS_EXIT="$child_status"
    trap - INT TERM
    return 0
}

# Record only presence, byte size and content hash; this does not validate image
# structure or certify completeness. Fixed suffixes prevent log-driven paths.
# Sets BEBLOG_MISSING_ARTIFACT_COUNT for the caller's terminal classification.
beblog_record_artifacts() {
    local prefix="$1" report_path="$2" suffix path
    BEBLOG_MISSING_ARTIFACT_COUNT=0
    for suffix in client.log raw params.txt tiff; do
        path="$prefix.$suffix"
        if [[ -f "$path" && ! -L "$path" ]]; then
            /usr/bin/stat -f 'artifact=%N bytes=%z' "$path" >> "$report_path"
            /usr/bin/shasum -a 256 "$path" >> "$report_path"
            printf 'artifact_validation=not_performed path=%s\n' "$path" >> "$report_path"
        else
            BEBLOG_MISSING_ARTIFACT_COUNT=$((BEBLOG_MISSING_ARTIFACT_COUNT + 1))
            printf 'artifact_missing_or_invalid=%s\n' "$path" >> "$report_path"
        fi
    done
}
