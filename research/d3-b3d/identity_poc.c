// Standalone macOS Security.framework experiment. Not linked into BeBlog.
#define _DARWIN_C_SOURCE 1
#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

extern char **environ;

// Trust anchor fixed by the research harness, not supplied by the helper,
// target path, or app UI. Script fails if rebuilding changes this test binary.
#define TRUSTED_TEST_CDHASH "bc2bb45947bc780002e91611c5661665064705ff"

static void report_osstatus(const char *where, OSStatus status) {
    fprintf(stderr, "%s: OSStatus=%d\n", where, (int)status);
}

static char *sha256_cdhash_from_info(CFDictionaryRef info) {
    CFNumberRef chosen = (CFNumberRef)CFDictionaryGetValue(info, kSecCodeInfoDigestAlgorithm);
    CFArrayRef algorithms = (CFArrayRef)CFDictionaryGetValue(info, kSecCodeInfoDigestAlgorithms);
    CFArrayRef hashes = (CFArrayRef)CFDictionaryGetValue(info, kSecCodeInfoCdHashes);
    int algorithm = 0;
    char *result = NULL;
    if (chosen && algorithms && hashes && CFNumberGetValue(chosen, kCFNumberIntType, &algorithm)
        && CFArrayGetCount(algorithms) == CFArrayGetCount(hashes)) {
        for (CFIndex i = 0; i < CFArrayGetCount(algorithms); i++) {
            int candidate = 0;
            CFNumberRef item = (CFNumberRef)CFArrayGetValueAtIndex(algorithms, i);
            if (!CFNumberGetValue(item, kCFNumberIntType, &candidate) || candidate != algorithm) continue;
            CFDataRef data = (CFDataRef)CFArrayGetValueAtIndex(hashes, i);
            // This is the conventional 20-byte CDHash derived from the
            // selected SHA-256 CodeDirectory digest.
            if (algorithm != kSecCodeSignatureHashSHA256 || CFDataGetLength(data) != 20) break;
            result = calloc(41, 1);
            if (!result) break;
            const UInt8 *bytes = CFDataGetBytePtr(data);
            for (int j = 0; j < 20; j++) snprintf(result + j * 2, 3, "%02x", bytes[j]);
            break;
        }
    }
    return result;
}

static char *sha256_cdhash_for_path(const char *path) {
    CFURLRef url = CFURLCreateFromFileSystemRepresentation(
        kCFAllocatorDefault, (const UInt8 *)path, (CFIndex)strlen(path), false);
    if (!url) return NULL;
    SecStaticCodeRef code = NULL;
    OSStatus status = SecStaticCodeCreateWithPath(url, kSecCSDefaultFlags, &code);
    CFRelease(url);
    if (status != errSecSuccess) { report_osstatus("static-code-create", status); return NULL; }
    status = SecStaticCodeCheckValidity(code, kSecCSStrictValidate, NULL);
    if (status != errSecSuccess) {
        report_osstatus("static-code-validity", status);
        CFRelease(code);
        return NULL;
    }
    CFDictionaryRef info = NULL;
    status = SecCodeCopySigningInformation(code, kSecCSSigningInformation, &info);
    CFRelease(code);
    if (status != errSecSuccess || !info) {
        report_osstatus("static-signing-information", status);
        return NULL;
    }
    char *result = sha256_cdhash_from_info(info);
    CFRelease(info);
    return result;
}

static bool validate_existing_guest(SecCodeRef guest, const char *expected_hex) {
    OSStatus status = SecCodeCheckValidity(guest, kSecCSDefaultFlags, NULL);
    if (status != errSecSuccess) {
        report_osstatus("dynamic-code-validity", status);
        return false;
    }
    CFDictionaryRef info = NULL;
    status = SecCodeCopySigningInformation(guest, kSecCSSigningInformation, &info);
    if (status != errSecSuccess || !info) {
        report_osstatus("dynamic-signing-information", status);
        return false;
    }
    char *actual_hex = sha256_cdhash_from_info(info);
    CFRelease(info);
    bool matches = actual_hex && expected_hex && strcmp(actual_hex, expected_hex) == 0;
    fprintf(stderr, "dynamic SHA-256 CDHash=%s expected=%s match=%s\n",
        actual_hex ? actual_hex : "unavailable", expected_hex ? expected_hex : "none", matches ? "yes" : "no");
    free(actual_hex);
    return matches;
}

static SecCodeRef identify_guest(pid_t pid, bool force_lookup_error) {
    pid_t lookup_pid = force_lookup_error ? (pid_t)-1 : pid;
    int pid_value = (int)lookup_pid;
    CFNumberRef number = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &pid_value);
    const void *keys[] = { kSecGuestAttributePid };
    const void *values[] = { number };
    CFDictionaryRef attrs = CFDictionaryCreate(kCFAllocatorDefault, keys, values, 1,
        &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
    CFRelease(number);
    SecCodeRef guest = NULL;
    OSStatus status = SecCodeCopyGuestWithAttributes(NULL, attrs, kSecCSDefaultFlags, &guest);
    CFRelease(attrs);
    if (status != errSecSuccess || !guest) { report_osstatus("guest-by-pid", status); return NULL; }
    return guest;
}

typedef struct {
    bool physically_reaped;
    bool unresolved_error;
} CleanupResult;

static CleanupResult terminate_and_reap(pid_t pid, bool inject_kill_error, bool inject_reap_error) {
    CleanupResult result = { .physically_reaped = false, .unresolved_error = false };
    if (inject_kill_error) {
        errno = EIO;
        perror("injected kill(SIGKILL) failure");
        result.unresolved_error = true;
        // Fault injection returns before the syscall. We still make a real
        // emergency kill while this unreaped child PID is owned by this parent.
    }
    if (kill(pid, SIGKILL) != 0 && errno != ESRCH) {
        perror("kill(SIGKILL)");
        result.unresolved_error = true;
        return result;
    }
    int status = 0;
    if (inject_reap_error) {
        errno = EIO;
        perror("injected waitpid failure");
        result.unresolved_error = true;
        // No actual wait occurred above; make one real wait now to prevent a
        // zombie. The injected observation error remains sticky for the gate.
    }
    pid_t waited;
    do { waited = waitpid(pid, &status, 0); } while (waited < 0 && errno == EINTR);
    if (waited != pid) { perror("waitpid after kill"); result.unresolved_error = true; return result; }
    result.physically_reaped = true;
    fprintf(stderr, "cleanup: SIGKILL requested; waitpid reaped pid=%d status=0x%x\n", pid, status);
    if (result.unresolved_error) fprintf(stderr, "cleanup verdict: unresolved injected operation failure; not complete\n");
    return result;
}

static int resume_child(pid_t pid, bool inject_failure) {
    if (inject_failure) { errno = EIO; return -1; }
    return kill(pid, SIGCONT);
}

static int spawn_suspended(const char *path, const char *marker, pid_t *pid_out) {
    posix_spawnattr_t attr;
    int error = posix_spawnattr_init(&attr);
    if (error) { errno = error; perror("posix_spawnattr_init"); return -1; }
    error = posix_spawnattr_setflags(&attr, POSIX_SPAWN_START_SUSPENDED);
    if (error) { errno = error; perror("posix_spawnattr_setflags"); posix_spawnattr_destroy(&attr); return -1; }
    char *const argv[] = { (char *)path, (char *)marker, NULL };
    error = posix_spawn(pid_out, path, NULL, &attr, argv, environ);
    int destroy_error = posix_spawnattr_destroy(&attr);
    if (error) { errno = error; perror("posix_spawn"); return -1; }
    if (destroy_error) {
        errno = destroy_error;
        perror("posix_spawnattr_destroy");
        (void)terminate_and_reap(*pid_out, false, false);
        return -1;
    }
    return 0;
}

static bool marker_exists(const char *marker) {
    return access(marker, F_OK) == 0;
}

static bool marker_contains(const char *marker, const char *expected_text) {
    char contents[128] = {0};
    int fd = open(marker, O_RDONLY);
    if (fd < 0) return false;
    ssize_t count = read(fd, contents, sizeof(contents) - 1);
    close(fd);
    return count >= 0 && strcmp(contents, expected_text) == 0;
}

typedef struct {
    bool spawned;
    bool suspended;
    bool identity_found;
    bool signature_valid;
    bool trusted_hash_matches;
    bool approved;
    bool continued;
    bool reaped;
    bool cleanup_complete;
    bool unexpected_end;
} GateObservation;

static bool gate_allows_success(GateObservation state) {
    return state.spawned && state.suspended && state.identity_found && state.signature_valid
        && state.trusted_hash_matches && state.approved && state.continued && state.reaped
        && state.cleanup_complete && !state.unexpected_end;
}

static int run_fault_gate_tests(void) {
    GateObservation good = { true, true, true, true, true, true, true, true, true, false };
    if (!gate_allows_success(good)) return -1;
    GateObservation cases[] = {
        { true, true, false, true, true, true, true, true, true, false }, // identity lookup
        { true, true, true, false, true, true, true, true, true, false }, // signature validation
        { true, true, true, true, false, true, true, true, true, false }, // pin mismatch
        { true, true, true, true, true, true, false, true, true, false }, // resume failure
        { true, true, true, true, true, true, false, true, false, false }, // kill failure
        { true, true, true, true, true, true, false, false, false, false }, // reap failure
        { true, true, true, true, true, true, true, true, true, true }, // unexpected end
    };
    for (size_t i = 0; i < sizeof(cases) / sizeof(cases[0]); i++) {
        if (gate_allows_success(cases[i])) return -1;
    }
    fprintf(stderr, "PASS: deterministic fault-gate tests reject every incomplete/error state\n");
    return 0;
}

static int wait_for_owned_child(pid_t pid, int *status) {
    pid_t waited;
    do { waited = waitpid(pid, status, 0); } while (waited < 0 && errno == EINTR);
    if (waited != pid) { perror("waitpid owned child"); return -1; }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 5) {
        fprintf(stderr, "usage: %s SCENARIO TARGET REPLACEMENT MARKER\n", argv[0]);
        return 64;
    }
    const char *scenario = argv[1];
    const char *target = argv[2];
    const char *replacement = argv[3];
    const char *marker = argv[4];
    if (run_fault_gate_tests() != 0) {
        fprintf(stderr, "FAIL: success gate accepted an injected fault\n");
        return 1;
    }
    bool unsigned_case = strcmp(scenario, "unsigned") == 0;
    char *expected = strdup(unsigned_case ? "0000000000000000000000000000000000000000" : TRUSTED_TEST_CDHASH);
    if (!expected) { fprintf(stderr, "FAIL-CLOSED: no trusted SHA-256 CDHash\n"); return 2; }
    if (!unsigned_case && strcmp(scenario, "spawn-failure") != 0) {
        char *preflight = sha256_cdhash_for_path(target);
        if (!preflight || strcmp(preflight, TRUSTED_TEST_CDHASH) != 0) {
            fprintf(stderr, "FAIL-CLOSED: fixed test helper does not match independent compiled-in trust anchor\n");
            free(preflight); free(expected); return 2;
        }
        free(preflight);
    }
    if (strcmp(scenario, "wrong-hash") == 0) expected[0] = expected[0] == '0' ? '1' : '0';

    if (strcmp(scenario, "tampered") == 0) {
        int fd = open(target, O_RDWR);
        UInt8 byte = 0;
        if (fd < 0 || pread(fd, &byte, 1, 4096) != 1) { perror("read byte for tamper"); if (fd >= 0) close(fd); free(expected); return 3; }
        byte ^= 0x01;
        if (pwrite(fd, &byte, 1, 4096) != 1) { perror("tamper target"); close(fd); free(expected); return 3; }
        close(fd);
    }
    if (strcmp(scenario, "remove-before-spawn") == 0) {
        if (unlink(target) != 0) { perror("unlink before spawn"); free(expected); return 3; }
    }
    if (strcmp(scenario, "swap-before-spawn") == 0) {
        if (rename(replacement, target) != 0) { perror("rename before spawn"); free(expected); return 3; }
    }

    pid_t pid = 0;
    bool expect_spawn_failure = strcmp(scenario, "spawn-failure") == 0 || strcmp(scenario, "remove-before-spawn") == 0;
    if (expect_spawn_failure) {
        if (spawn_suspended(target, marker, &pid) == 0) {
            fprintf(stderr, "FAIL: spawn unexpectedly succeeded after path absence\n");
            (void)terminate_and_reap(pid, false, false); free(expected); return 4;
        }
        fprintf(stderr, "PASS: path absence failed spawn closed\n");
        free(expected);
        return 0;
    }
    if (spawn_suspended(target, marker, &pid) != 0) { free(expected); return 4; }
    fprintf(stderr, "spawned suspended pid=%d scenario=%s\n", pid, scenario);
    if (marker_exists(marker)) {
        fprintf(stderr, "FAIL: marker exists before approval\n");
        (void)terminate_and_reap(pid, false, false); free(expected); return 5;
    }

    if (strcmp(scenario, "swap-after-spawn") == 0) {
        if (rename(replacement, target) != 0) { perror("rename after spawn"); (void)terminate_and_reap(pid, false, false); free(expected); return 3; }
    }
    if (strcmp(scenario, "unexpected-end") == 0) {
        if (kill(pid, SIGKILL) != 0) { perror("kill unexpected-end child"); (void)terminate_and_reap(pid, false, false); free(expected); return 3; }
        // Keep the exited child unreaped so the PID cannot be reused before
        // testing whether Security.framework still exposes dynamic code.
        SecCodeRef after_end = identify_guest(pid, false);
        bool still_valid = after_end && validate_existing_guest(after_end, expected);
        if (after_end) CFRelease(after_end);
        int status = 0;
        if (wait_for_owned_child(pid, &status) != 0) { free(expected); return 3; }
        fprintf(stderr, "unexpected-end reaped child status=0x%x dynamic-valid=%s; no PID use after reap\n", status, still_valid ? "yes" : "no");
        free(expected);
        fprintf(stderr, "PASS: ended child was not resumed; result is denied regardless of lingering SecCode validity\n");
        return 0;
    }

    bool lookup_error = strcmp(scenario, "lookup-failure") == 0 || strcmp(scenario, "kill-failure") == 0 || strcmp(scenario, "reap-failure") == 0;
    SecCodeRef guest = identify_guest(pid, lookup_error);
    bool matched = guest && validate_existing_guest(guest, expected);
    if (strcmp(scenario, "tampered") == 0 || strcmp(scenario, "unsigned") == 0 || strcmp(scenario, "wrong-hash") == 0
        || strcmp(scenario, "swap-before-spawn") == 0 || strcmp(scenario, "swap-after-spawn") == 0
        || strcmp(scenario, "lookup-failure") == 0 || strcmp(scenario, "kill-failure") == 0
        || strcmp(scenario, "reap-failure") == 0) {
        if (matched) { fprintf(stderr, "FAIL: expected identity/signature rejection\n"); if (guest) CFRelease(guest); (void)terminate_and_reap(pid, false, false); free(expected); return 7; }
        bool inject_kill = strcmp(scenario, "kill-failure") == 0;
        bool inject_reap = strcmp(scenario, "reap-failure") == 0;
        CleanupResult cleanup = terminate_and_reap(pid, inject_kill, inject_reap);
        if (guest) CFRelease(guest);
        if (!cleanup.physically_reaped || cleanup.unresolved_error != (inject_kill || inject_reap)) {
            fprintf(stderr, "FAIL: cleanup fault result misclassified\n"); free(expected); return 8;
        }
        if (marker_exists(marker)) { fprintf(stderr, "FAIL: rejected program executed\n"); free(expected); return 9; }
        fprintf(stderr, "PASS: rejected before continue; cleanup physically_reaped=%d unresolved_error=%d\n",
            cleanup.physically_reaped, cleanup.unresolved_error);
        free(expected);
        return 0;
    }

    if (!matched) {
        if (guest) CFRelease(guest);
        (void)terminate_and_reap(pid, false, false); free(expected); return 10;
    }

    bool swap_after_validity = strcmp(scenario, "swap-after-validity") == 0;
    bool remove_after_validity = strcmp(scenario, "remove-after-validity") == 0;
    if (swap_after_validity) {
        if (rename(replacement, target) != 0) { perror("rename after successful validation"); CFRelease(guest); (void)terminate_and_reap(pid, false, false); free(expected); return 11; }
        fprintf(stderr, "path replaced after first successful dynamic validation\n");
    }
    if (remove_after_validity) {
        if (unlink(target) != 0) { perror("unlink after successful validation"); CFRelease(guest); (void)terminate_and_reap(pid, false, false); free(expected); return 12; }
        fprintf(stderr, "original pathname removed after first successful dynamic validation\n");
    }
    bool second_valid = validate_existing_guest(guest, expected);
    if ((swap_after_validity && !second_valid) || (remove_after_validity && !second_valid)) {
        CleanupResult cleanup = terminate_and_reap(pid, false, false);
        CFRelease(guest);
        bool marker_absent = !marker_exists(marker);
        fprintf(stderr, "path-change gate result: reaped=%d marker_absent=%d\n", cleanup.physically_reaped, marker_absent);
        free(expected);
        return cleanup.physically_reaped && !cleanup.unresolved_error && marker_absent ? 0 : 14;
    }
    if (!second_valid) { CFRelease(guest); (void)terminate_and_reap(pid, false, false); free(expected); return 15; }

    if (strcmp(scenario, "abort-before-continue") == 0) {
        CleanupResult cleanup = terminate_and_reap(pid, false, false);
        CFRelease(guest);
        bool marker_absent = !marker_exists(marker);
        fprintf(stderr, "PASS: abort before continue; reaped=%d marker_absent=%d\n", cleanup.physically_reaped, marker_absent);
        free(expected);
        return cleanup.physically_reaped && marker_absent ? 0 : 16;
    }

    if (strcmp(scenario, "resume-failure") == 0) {
        errno = 0;
        if (resume_child(pid, true) == 0 || errno != EIO) {
            fprintf(stderr, "FAIL: simulated resume failure not observed\n"); CFRelease(guest); (void)terminate_and_reap(pid, false, false); free(expected); return 17;
        }
        fprintf(stderr, "injected SIGCONT failure returned EIO\n");
        CleanupResult cleanup = terminate_and_reap(pid, false, false);
        CFRelease(guest);
        bool marker_absent = !marker_exists(marker);
        fprintf(stderr, "PASS: failed continuation denied; reaped=%d marker_absent=%d\n", cleanup.physically_reaped, marker_absent);
        free(expected);
        return cleanup.physically_reaped && !cleanup.unresolved_error && marker_absent ? 0 : 18;
    }

    int alive_status = 0;
    pid_t alive = waitpid(pid, &alive_status, WNOHANG);
    if (alive != 0) {
        // If waitpid reports the child exited, it is now reaped; do not signal
        // this numeric PID again. A running child returns 0 and remains ours.
        if (alive == pid) fprintf(stderr, "unexpected child exit before resume; reaped status=0x%x\n", alive_status);
        else perror("waitpid(WNOHANG) before resume");
        CFRelease(guest); free(expected); return 19;
    }
    if (kill(pid, SIGCONT) != 0) {
        perror("kill(SIGCONT)"); CleanupResult cleanup = terminate_and_reap(pid, false, false);
        CFRelease(guest); free(expected); return cleanup.physically_reaped ? 20 : 21;
    }
    fprintf(stderr, "SIGCONT sent after second dynamic validation; pid=%d\n", pid);
    int status = 0;
    if (wait_for_owned_child(pid, &status) != 0) { CFRelease(guest); free(expected); return 22; }
    CFRelease(guest);
    bool ran = marker_exists(marker);
    bool original_image_ran = marker_contains(marker, "child-A-ran\n");
    fprintf(stderr, "PASS: identity accepted; child reaped status=0x%x marker=%s original-image=%s\n",
        status, ran ? "present" : "missing", original_image_ran ? "confirmed" : "not-confirmed");
    if (remove_after_validity) {
        fprintf(stderr, "OBSERVED: removal case final status=%s, marker=%s; dynamic identity alone was not treated as successful execution\n",
            WIFSIGNALED(status) ? "signal-terminated" : (WIFEXITED(status) ? "exited" : "other"), ran ? "present" : "absent");
        free(expected);
        return 0;
    }
    free(expected);
    return ran && original_image_ran && WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 16;
}
