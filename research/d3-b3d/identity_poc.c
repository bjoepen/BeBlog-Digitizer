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

static bool live_guest_matches(pid_t pid, const char *expected_hex, bool force_lookup_error) {
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
    if (status != errSecSuccess || !guest) { report_osstatus("guest-by-pid", status); return false; }
    // Dynamic validation is deliberately performed before extracting the
    // signature data; Apple's API contract says this protects against changes
    // to the filesystem source of the running SecCode object.
    status = SecCodeCheckValidity(guest, kSecCSDefaultFlags, NULL);
    if (status != errSecSuccess) {
        report_osstatus("dynamic-code-validity", status);
        CFRelease(guest);
        return false;
    }
    CFDictionaryRef info = NULL;
    status = SecCodeCopySigningInformation(guest, kSecCSSigningInformation, &info);
    CFRelease(guest);
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

static int terminate_and_reap(pid_t pid) {
    if (kill(pid, SIGKILL) != 0 && errno != ESRCH) {
        perror("kill(SIGKILL)");
        return -1;
    }
    int status = 0;
    pid_t waited;
    do { waited = waitpid(pid, &status, 0); } while (waited < 0 && errno == EINTR);
    if (waited != pid) { perror("waitpid after kill"); return -1; }
    fprintf(stderr, "cleanup: SIGKILL requested; waitpid reaped pid=%d status=0x%x\n", pid, status);
    return 0;
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
        terminate_and_reap(*pid_out);
        return -1;
    }
    return 0;
}

static bool marker_exists(const char *marker) {
    return access(marker, F_OK) == 0;
}

int main(int argc, char **argv) {
    if (argc != 6) {
        fprintf(stderr, "usage: %s SCENARIO TARGET TRUSTED_REFERENCE REPLACEMENT MARKER\n", argv[0]);
        return 64;
    }
    const char *scenario = argv[1];
    const char *target = argv[2];
    const char *trusted_reference = argv[3];
    const char *replacement = argv[4];
    const char *marker = argv[5];
    bool unsigned_case = strcmp(scenario, "unsigned") == 0;
    char *expected = unsigned_case ? strdup("0000000000000000000000000000000000000000") : sha256_cdhash_for_path(trusted_reference);
    if (!expected) { fprintf(stderr, "FAIL-CLOSED: no trusted SHA-256 CDHash\n"); return 2; }
    if (strcmp(scenario, "wrong-hash") == 0) expected[0] = expected[0] == '0' ? '1' : '0';

    if (strcmp(scenario, "tampered") == 0) {
        int fd = open(target, O_RDWR);
        UInt8 byte = 0;
        if (fd < 0 || pread(fd, &byte, 1, 4096) != 1) { perror("read byte for tamper"); if (fd >= 0) close(fd); free(expected); return 3; }
        byte ^= 0x01;
        if (pwrite(fd, &byte, 1, 4096) != 1) { perror("tamper target"); close(fd); free(expected); return 3; }
        close(fd);
    }
    if (strcmp(scenario, "swap-before-spawn") == 0) {
        if (rename(replacement, target) != 0) { perror("rename before spawn"); free(expected); return 3; }
    }

    pid_t pid = 0;
    if (strcmp(scenario, "spawn-failure") == 0) {
        if (spawn_suspended(target, marker, &pid) == 0) {
            fprintf(stderr, "FAIL: absent-path spawn unexpectedly succeeded\n");
            terminate_and_reap(pid); free(expected); return 4;
        }
        fprintf(stderr, "PASS: nonexistent fixed test path failed to spawn\n");
        free(expected);
        return 0;
    }
    if (spawn_suspended(target, marker, &pid) != 0) { free(expected); return 4; }
    fprintf(stderr, "spawned suspended pid=%d scenario=%s\n", pid, scenario);
    if (marker_exists(marker)) {
        fprintf(stderr, "FAIL: marker exists before approval\n");
        terminate_and_reap(pid); free(expected); return 5;
    }

    if (strcmp(scenario, "swap-after-spawn") == 0) {
        if (rename(replacement, target) != 0) { perror("rename after spawn"); terminate_and_reap(pid); free(expected); return 3; }
    }
    if (strcmp(scenario, "unexpected-end") == 0) {
        if (kill(pid, SIGKILL) != 0) { perror("kill unexpected-end child"); terminate_and_reap(pid); free(expected); return 3; }
        int status = 0;
        if (waitpid(pid, &status, 0) != pid) { perror("waitpid unexpected-end child"); free(expected); return 3; }
        fprintf(stderr, "unexpected-end injection reaped child status=0x%x\n", status);
        bool identified = live_guest_matches(pid, expected, false);
        fprintf(stderr, "identity-after-end=%s\n", identified ? "unexpectedly succeeded" : "rejected");
        free(expected);
        return identified ? 6 : 0;
    }

    bool lookup_error = strcmp(scenario, "lookup-failure") == 0;
    bool matched = live_guest_matches(pid, expected, lookup_error);
    bool should_accept = strcmp(scenario, "success") == 0 || strcmp(scenario, "resume-abort") == 0;
    if (matched != should_accept) {
        fprintf(stderr, "FAIL: identity matched=%d, expected acceptance=%d\n", matched, should_accept);
        terminate_and_reap(pid); free(expected); return 7;
    }

    if (!matched) {
        if (terminate_and_reap(pid) != 0) { free(expected); return 8; }
        if (marker_exists(marker)) { fprintf(stderr, "FAIL: denied program executed\n"); free(expected); return 9; }
        fprintf(stderr, "PASS: rejected identity; program never ran\n");
        free(expected);
        return 0;
    }

    if (strcmp(scenario, "resume-abort") == 0) {
        if (kill(pid, SIGKILL) != 0) { perror("kill resume-failure child"); terminate_and_reap(pid); free(expected); return 10; }
        int status = 0;
        if (waitpid(pid, &status, 0) != pid) { perror("waitpid before resume"); free(expected); return 11; }
        // Do not signal a PID after reaping: it could already have been reused.
        // This safely exercises the failure branch that aborts before SIGCONT.
        fprintf(stderr, "PASS: fail-closed abort before resume after kill+reap; marker=%s\n", marker_exists(marker) ? "unexpectedly present" : "absent");
        free(expected);
        return marker_exists(marker) ? 13 : 0;
    }

    if (kill(pid, SIGCONT) != 0) {
        perror("kill(SIGCONT)"); terminate_and_reap(pid); free(expected); return 14;
    }
    int status = 0;
    pid_t waited;
    do { waited = waitpid(pid, &status, 0); } while (waited < 0 && errno == EINTR);
    if (waited != pid) {
        // A failed wait can leave PID ownership ambiguous. Do not send a signal
        // by numeric PID after that point; report cleanup as unresolved.
        perror("waitpid after continue; no post-wait PID signal attempted");
        free(expected);
        return 15;
    }
    bool ran = marker_exists(marker);
    fprintf(stderr, "PASS: identity accepted; child reaped status=0x%x marker=%s\n", status, ran ? "present" : "missing");
    free(expected);
    return ran && WIFEXITED(status) && WEXITSTATUS(status) == 0 ? 0 : 16;
}
