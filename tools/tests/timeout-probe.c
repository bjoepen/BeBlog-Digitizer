// Harmless process used only by shell timeout regression tests; never uses SANE.
#define _POSIX_C_SOURCE 200809L
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

static volatile sig_atomic_t received_term = 0;

static void mark_term(int signal_number) {
    (void)signal_number;
    received_term = 1;
}

int main(int argc, char **argv) {
    if (argc != 3) return 90;
    if (strcmp(argv[1], "cooperative") == 0) {
        struct sigaction action;
        memset(&action, 0, sizeof action);
        action.sa_handler = mark_term;
        sigemptyset(&action.sa_mask);
        if (sigaction(SIGTERM, &action, NULL) != 0) return 93;
    } else if (strcmp(argv[1], "ignore-term") == 0) {
        signal(SIGTERM, SIG_IGN);
    }
    int marker = open(argv[2], O_WRONLY | O_CREAT | O_APPEND, 0600);
    if (marker < 0) return 91;
    if (write(marker, "started\n", 8) != 8) return 92;
    close(marker);

    if (strcmp(argv[1], "success") == 0) return 0;
    if (strcmp(argv[1], "exit7") == 0) return 7;
    if (strcmp(argv[1], "cooperative") == 0) {
        const struct timespec delay = {0, 10000000};
        while (!received_term) nanosleep(&delay, NULL);
        return 0;
    }
    if (strcmp(argv[1], "ignore-term") == 0) {
        for (;;) pause();
    }
    return 94;
}
