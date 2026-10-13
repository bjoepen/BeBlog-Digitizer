#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#ifndef CHILD_MARKER_TEXT
#define CHILD_MARKER_TEXT "child-ran"
#endif

int main(int argc, char **argv) {
    if (argc != 2) return 90;
    int fd = open(argv[1], O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) return 91;
    const char *text = CHILD_MARKER_TEXT "\n";
    size_t left = strlen(text);
    while (left > 0) {
        ssize_t n = write(fd, text, left);
        if (n <= 0) { close(fd); return 92; }
        text += n;
        left -= (size_t)n;
    }
    close(fd);
    return 0;
}
