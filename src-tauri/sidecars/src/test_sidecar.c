#define _DARWIN_C_SOURCE
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int write_marker(const char *requested_dir) {
    char temp_root[PATH_MAX];
    char marker_dir[PATH_MAX];
    const char *tmp = getenv("TMPDIR");
    if (tmp == NULL || realpath(tmp, temp_root) == NULL ||
        realpath(requested_dir, marker_dir) == NULL) {
        return 64;
    }
    size_t root_len = strlen(temp_root);
    if (strncmp(marker_dir, temp_root, root_len) != 0 ||
        (marker_dir[root_len] != '/' && marker_dir[root_len] != '\0')) {
        fprintf(stderr, "refusing marker path outside TMPDIR\n");
        return 65;
    }
    int dir_fd = open(marker_dir, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (dir_fd < 0) return 66;
    int marker_fd = openat(dir_fd, "beblog-test-sidecar.marker",
                           O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
                           0600);
    if (marker_fd < 0) {
        close(dir_fd);
        return 67;
    }
    static const char marker[] = "beblog-arm64-test-sidecar\n";
    ssize_t written = write(marker_fd, marker, sizeof(marker) - 1);
    int result = (written == (ssize_t)(sizeof(marker) - 1) && fsync(marker_fd) == 0) ? 0 : 68;
    close(marker_fd);
    close(dir_fd);
    return result;
}

int main(int argc, char **argv) {
    if (argc == 1) {
        puts("arch=arm64 id=beblog-test-sidecar-v1");
        return 0;
    }
    if (argc == 3 && strcmp(argv[1], "--marker-dir") == 0) {
        puts("arch=arm64 id=beblog-test-sidecar-v1");
        return write_marker(argv[2]);
    }
    fprintf(stderr, "usage: beblog-test-sidecar [--marker-dir TEMP_DIRECTORY]\n");
    return 64;
}
