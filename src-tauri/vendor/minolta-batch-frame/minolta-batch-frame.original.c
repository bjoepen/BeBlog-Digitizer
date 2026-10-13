#define _POSIX_C_SOURCE 200809L

#include <sane/sane.h>
#include <sane/saneopts.h>

#include <errno.h>
#include <signal.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define READ_BUFFER_SIZE (256U * 1024U)
#define DEFAULT_POLL_SECONDS 2U
#define DEFAULT_TIMEOUT_SECONDS 120U

static volatile sig_atomic_t interrupted = 0;
static FILE *log_file;

static void on_signal(int signo) { (void)signo; interrupted = 1; }

static void timestamp(char out[32]) {
    struct timespec ts;
    struct tm tm;
    clock_gettime(CLOCK_REALTIME, &ts);
    localtime_r(&ts.tv_sec, &tm);
    strftime(out, 24, "%Y-%m-%dT%H:%M:%S", &tm);
    snprintf(out + strlen(out), 32U - strlen(out), ".%03ld", ts.tv_nsec / 1000000L);
}

static void diag(const char *fmt, ...) {
    char ts[32];
    va_list ap;
    timestamp(ts);
    fprintf(stderr, "[%s] ", ts);
    if (log_file) fprintf(log_file, "[%s] ", ts);
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    va_start(ap, fmt);
    if (log_file) { vfprintf(log_file, fmt, ap); fflush(log_file); }
    va_end(ap);
    fflush(stderr);
}

static void log_status(const char *call, SANE_Status st) {
    diag("%s -> %d (%s)\n", call, (int)st, sane_strstatus(st));
}

static bool prompt_exact(const char *message, const char *expected) {
    char line[128];
    fprintf(stderr, "\n%s\nEingabe '%s' zum Fortfahren: ", message, expected);
    fflush(stderr);
    if (!fgets(line, sizeof line, stdin)) return false;
    line[strcspn(line, "\r\n")] = '\0';
    return strcmp(line, expected) == 0;
}

static int find_option(SANE_Handle h, const char *name) {
    SANE_Int count = 0;
    SANE_Status st = sane_control_option(h, 0, SANE_ACTION_GET_VALUE, &count, NULL);
    log_status("sane_control_option(option-count, GET)", st);
    if (st != SANE_STATUS_GOOD) return -1;
    for (SANE_Int i = 1; i < count; ++i) {
        const SANE_Option_Descriptor *d = sane_get_option_descriptor(h, i);
        diag("sane_get_option_descriptor(%d) -> %s\n", (int)i, d ? "descriptor" : "NULL");
        if (d && d->name && strcmp(d->name, name) == 0) return (int)i;
    }
    return -1;
}

static SANE_Status set_word(SANE_Handle h, int option, SANE_Word value, const char *label) {
    SANE_Int info = 0;
    SANE_Status st = sane_control_option(h, option, SANE_ACTION_SET_VALUE, &value, &info);
    diag("sane_control_option(%s, SET=%d) -> %d (%s), info=0x%x\n",
         label, (int)value, (int)st, sane_strstatus(st), (unsigned)info);
    return st;
}

static SANE_Status set_string(SANE_Handle h, int option, const char *value, const char *label) {
    const SANE_Option_Descriptor *d = sane_get_option_descriptor(h, option);
    diag("sane_get_option_descriptor(%s) -> %s\n", label, d ? "descriptor" : "NULL");
    if (!d || d->size <= 0) return SANE_STATUS_INVAL;
    char *copy = calloc(1, (size_t)d->size);
    if (!copy) return SANE_STATUS_NO_MEM;
    snprintf(copy, (size_t)d->size, "%s", value);
    SANE_Int info = 0;
    SANE_Status st = sane_control_option(h, option, SANE_ACTION_SET_VALUE, copy, &info);
    diag("sane_control_option(%s, SET='%s') -> %d (%s), info=0x%x\n",
         label, value, (int)st, sane_strstatus(st), (unsigned)info);
    free(copy);
    return st;
}

static bool frame_range(const SANE_Option_Descriptor *d, long *min, long *max, long *quant) {
    if (!d || d->type != SANE_TYPE_INT || d->constraint_type != SANE_CONSTRAINT_RANGE ||
        !d->constraint.range) return false;
    *min = d->constraint.range->min;
    *max = d->constraint.range->max;
    *quant = d->constraint.range->quant;
    return true;
}

static SANE_Status wait_for_holder(SANE_Handle h, int frame_opt, unsigned timeout, unsigned interval) {
    const time_t deadline = time(NULL) + (time_t)timeout;
    unsigned poll = 0;
    while (!interrupted && time(NULL) <= deadline) {
        SANE_Word current = 0;
        SANE_Int info = 0;
        SANE_Status st = sane_control_option(h, frame_opt, SANE_ACTION_GET_VALUE, &current, &info);
        diag("holder poll %u: sane_control_option(frame, GET) -> %d (%s), value=%d, info=0x%x\n",
             ++poll, (int)st, sane_strstatus(st), (int)current, (unsigned)info);
        if (st != SANE_STATUS_GOOD) return st;

        const SANE_Option_Descriptor *d = sane_get_option_descriptor(h, frame_opt);
        long min = 0, max = 0, quant = 0;
        if (frame_range(d, &min, &max, &quant)) {
            diag("holder poll %u: frame descriptor range=%ld..%ld quant=%ld\n",
                 poll, min, max, quant);
            if (max > 1) return SANE_STATUS_GOOD;
        } else {
            diag("holder poll %u: frame descriptor has no integer range\n", poll);
        }
        if (interval) sleep(interval);
    }
    return interrupted ? SANE_STATUS_CANCELLED : SANE_STATUS_DEVICE_BUSY;
}

static const char *frame_name(SANE_Frame f) {
    switch (f) {
        case SANE_FRAME_GRAY: return "GRAY";
        case SANE_FRAME_RGB: return "RGB";
        case SANE_FRAME_RED: return "RED";
        case SANE_FRAME_GREEN: return "GREEN";
        case SANE_FRAME_BLUE: return "BLUE";
        default: return "OTHER";
    }
}

static bool write_metadata(const char *path, const char *device, const SANE_Parameters *p,
                           uint64_t bytes, SANE_Status terminal) {
    FILE *f = fopen(path, "wb");
    if (!f) { diag("fopen(%s) failed: %s\n", path, strerror(errno)); return false; }
    fprintf(f, "device=%s\nformat=%s\nformat_code=%d\nlast_frame=%d\nbytes_per_line=%d\n"
               "pixels_per_line=%d\nlines=%d\ndepth=%d\nraw_bytes=%llu\nterminal_status=%d\n"
               "terminal_status_text=%s\n",
            device, frame_name(p->format), (int)p->format, (int)p->last_frame,
            (int)p->bytes_per_line, (int)p->pixels_per_line, (int)p->lines, (int)p->depth,
            (unsigned long long)bytes, (int)terminal, sane_strstatus(terminal));
    bool ok = fclose(f) == 0;
    diag("metadata %s -> %s\n", path, ok ? "written" : "close failed");
    return ok;
}

static void put16(FILE *f, uint16_t v) {
    unsigned char b[2] = {(unsigned char)(v & 255U), (unsigned char)(v >> 8)};
    fwrite(b, 1, 2, f);
}
static void put32(FILE *f, uint32_t v) {
    unsigned char b[4] = {(unsigned char)(v & 255U), (unsigned char)((v >> 8) & 255U),
                          (unsigned char)((v >> 16) & 255U), (unsigned char)(v >> 24)};
    fwrite(b, 1, 4, f);
}
static void ifd(FILE *f, uint16_t tag, uint16_t type, uint32_t count, uint32_t value) {
    put16(f, tag); put16(f, type); put32(f, count); put32(f, value);
}

/* Creates an uncompressed baseline TIFF from the exact raw capture when layout is supported. */
static bool raw_to_tiff(const char *raw_path, const char *tiff_path, const SANE_Parameters *p) {
    if (p->lines <= 0 || p->pixels_per_line <= 0 || (p->depth != 8 && p->depth != 16) ||
        (p->format != SANE_FRAME_GRAY && p->format != SANE_FRAME_RGB)) {
        diag("TIFF skipped: unsupported/unknown parameters; raw capture remains authoritative\n");
        return false;
    }
    const uint32_t samples = p->format == SANE_FRAME_RGB ? 3U : 1U;
    const uint64_t row_bytes64 = (uint64_t)p->pixels_per_line * samples * (uint32_t)p->depth / 8U;
    const uint64_t image_bytes64 = row_bytes64 * (uint32_t)p->lines;
    if (row_bytes64 > (uint64_t)p->bytes_per_line || row_bytes64 > UINT32_MAX || image_bytes64 > UINT32_MAX) {
        diag("TIFF skipped: inconsistent or >4 GiB classic-TIFF layout\n");
        return false;
    }

    FILE *in = fopen(raw_path, "rb");
    FILE *out = fopen(tiff_path, "wb");
    if (!in || !out) {
        diag("TIFF open failed: %s\n", strerror(errno));
        if (in) fclose(in); if (out) fclose(out); return false;
    }
    const uint16_t entries = p->format == SANE_FRAME_RGB ? 11U : 10U;
    const uint32_t ifd_offset = 8U;
    const uint32_t extras = ifd_offset + 2U + (uint32_t)entries * 12U + 4U;
    const uint32_t bits_offset = extras;
    const uint32_t pixel_offset = extras + (samples == 3U ? 6U : 0U);
    fwrite("II", 1, 2, out); put16(out, 42); put32(out, ifd_offset); put16(out, entries);
    ifd(out, 256, 4, 1, (uint32_t)p->pixels_per_line);
    ifd(out, 257, 4, 1, (uint32_t)p->lines);
    ifd(out, 258, 3, samples, samples == 1U ? (uint32_t)p->depth : bits_offset);
    ifd(out, 259, 3, 1, 1);
    ifd(out, 262, 3, 1, samples == 3U ? 2U : 1U);
    ifd(out, 273, 4, 1, pixel_offset);
    ifd(out, 277, 3, 1, samples);
    ifd(out, 278, 4, 1, (uint32_t)p->lines);
    ifd(out, 279, 4, 1, (uint32_t)image_bytes64);
    ifd(out, 284, 3, 1, 1);
    if (samples == 3U) ifd(out, 339, 3, samples, bits_offset + 6U + (uint32_t)image_bytes64);
    put32(out, 0);
    if (samples == 3U) { put16(out, (uint16_t)p->depth); put16(out, (uint16_t)p->depth); put16(out, (uint16_t)p->depth); }

    unsigned char *row = malloc((size_t)p->bytes_per_line);
    bool ok = row != NULL;
    for (int y = 0; ok && y < p->lines; ++y) {
        ok = fread(row, 1, (size_t)p->bytes_per_line, in) == (size_t)p->bytes_per_line;
        if (ok && p->depth == 16) {
            /* SANE 16-bit samples are native-endian; TIFF is explicitly little-endian here. */
#if __BYTE_ORDER__ == __ORDER_BIG_ENDIAN__
            for (size_t x = 0; x + 1 < (size_t)row_bytes64; x += 2) {
                unsigned char t = row[x]; row[x] = row[x + 1]; row[x + 1] = t;
            }
#endif
        }
        if (ok) ok = fwrite(row, 1, (size_t)row_bytes64, out) == (size_t)row_bytes64;
    }
    /* SampleFormat=UnsignedInteger array, located after pixel data. */
    if (ok && samples == 3U) { put16(out, 1); put16(out, 1); put16(out, 1); }
    free(row);
    if (ferror(in) || ferror(out)) ok = false;
    int in_close = fclose(in);
    int out_close = fclose(out);
    if (in_close != 0 || out_close != 0) ok = false;
    diag("TIFF %s -> %s\n", tiff_path, ok ? "written" : "FAILED (raw retained)");
    return ok;
}

static void usage(const char *argv0) {
    fprintf(stderr, "Usage: %s -d DEVICE [-o PREFIX] [-f FRAME] [-t SECONDS] [-i SECONDS]\n"
                    "Example: SANE_DEBUG_AVISION=9 %s -d 'avision:libusb:002:006' -o frame1\n",
            argv0, argv0);
}

int main(int argc, char **argv) {
    const char *device = NULL;
    const char *prefix = "minolta-batch-frame";
    unsigned frame = 1;
    unsigned timeout = DEFAULT_TIMEOUT_SECONDS, interval = DEFAULT_POLL_SECONDS;
    int ch;
    while ((ch = getopt(argc, argv, "d:o:f:t:i:h")) != -1) {
        switch (ch) {
            case 'd': device = optarg; break;
            case 'o': prefix = optarg; break;
            case 'f': frame = (unsigned)strtoul(optarg, NULL, 10); break;
            case 't': timeout = (unsigned)strtoul(optarg, NULL, 10); break;
            case 'i': interval = (unsigned)strtoul(optarg, NULL, 10); break;
            default: usage(argv[0]); return ch == 'h' ? 0 : 2;
        }
    }
    if (!device || frame < 1 || frame > 4 || timeout == 0 || interval == 0 || interval > timeout) { usage(argv[0]); return 2; }

    char log_path[1024], raw_path[1024], meta_path[1024], tiff_path[1024];
    if (snprintf(log_path, sizeof log_path, "%s.client.log", prefix) >= (int)sizeof log_path ||
        snprintf(raw_path, sizeof raw_path, "%s.raw", prefix) >= (int)sizeof raw_path ||
        snprintf(meta_path, sizeof meta_path, "%s.params.txt", prefix) >= (int)sizeof meta_path ||
        snprintf(tiff_path, sizeof tiff_path, "%s.tiff", prefix) >= (int)sizeof tiff_path) return 2;
    log_file = fopen(log_path, "wb");
    if (!log_file) { fprintf(stderr, "Cannot create %s: %s\n", log_path, strerror(errno)); return 1; }
    signal(SIGINT, on_signal); signal(SIGTERM, on_signal);
    const char *debug_level = getenv("SANE_DEBUG_AVISION");
    diag("client start; device=%s timeout=%u interval=%u SANE_DEBUG_AVISION=%s\n",
         device, timeout, interval, debug_level ? debug_level : "(unset)");

    diag("batch mode: film holder remains inserted; requested frame=%u\n", frame);

    SANE_Int version = 0;
    SANE_Status st = sane_init(&version, NULL);
    log_status("sane_init", st);
    if (st != SANE_STATUS_GOOD) { fclose(log_file); return 1; }
    diag("SANE version code=0x%x\n", (unsigned)version);

    SANE_Handle h = NULL;
    bool scan_active = false;
    st = sane_open(device, &h);
    log_status("sane_open (only call)", st);
    if (st != SANE_STATUS_GOOD) goto done;

    int frame_opt = find_option(h, "frame");
    int mode_opt = find_option(h, SANE_NAME_SCAN_MODE);
    int resolution_opt = find_option(h, SANE_NAME_SCAN_RESOLUTION);
    if (frame_opt < 0 || mode_opt < 0 || resolution_opt < 0) {
        diag("required option missing: frame=%d mode=%d resolution=%d\n", frame_opt, mode_opt, resolution_opt);
        st = SANE_STATUS_UNSUPPORTED; goto done;
    }

    st = wait_for_holder(h, frame_opt, timeout, interval);
    log_status("wait_for_holder", st);
    if (st != SANE_STATUS_GOOD) goto done;

    if ((st = set_word(h, frame_opt, (SANE_Word)frame, "frame")) != SANE_STATUS_GOOD ||
        (st = set_string(h, mode_opt, SANE_VALUE_SCAN_MODE_COLOR, "mode")) != SANE_STATUS_GOOD ||
        (st = set_word(h, resolution_opt, 600, "resolution")) != SANE_STATUS_GOOD) goto done;

    SANE_Parameters params;
    memset(&params, 0, sizeof params);
    st = sane_get_parameters(h, &params);
    log_status("sane_get_parameters(before start)", st);
    if (st != SANE_STATUS_GOOD) goto done;
    diag("parameters(pre): format=%s last=%d bpl=%d pixels=%d lines=%d depth=%d\n",
         frame_name(params.format), params.last_frame, params.bytes_per_line,
         params.pixels_per_line, params.lines, params.depth);

    st = sane_start(h);
    log_status("sane_start (only call)", st);
    if (st != SANE_STATUS_GOOD) goto done;
    scan_active = true;

    st = sane_get_parameters(h, &params);
    log_status("sane_get_parameters(after start)", st);
    if (st != SANE_STATUS_GOOD) goto done;
    diag("parameters(post): format=%s last=%d bpl=%d pixels=%d lines=%d depth=%d\n",
         frame_name(params.format), params.last_frame, params.bytes_per_line,
         params.pixels_per_line, params.lines, params.depth);

    FILE *raw = fopen(raw_path, "wb");
    if (!raw) { diag("fopen(%s) failed: %s\n", raw_path, strerror(errno)); st = SANE_STATUS_IO_ERROR; goto done; }
    unsigned char *buf = malloc(READ_BUFFER_SIZE);
    if (!buf) { fclose(raw); st = SANE_STATUS_NO_MEM; goto done; }
    uint64_t total = 0;
    SANE_Status terminal = SANE_STATUS_GOOD;
    while (!interrupted) {
        SANE_Int got = 0;
        terminal = sane_read(h, buf, (SANE_Int)READ_BUFFER_SIZE, &got);
        diag("sane_read -> %d (%s), bytes=%d\n", (int)terminal, sane_strstatus(terminal), (int)got);
        if (got > 0) {
            if (fwrite(buf, 1, (size_t)got, raw) != (size_t)got) { terminal = SANE_STATUS_IO_ERROR; break; }
            total += (uint64_t)got;
        }
        if (terminal == SANE_STATUS_EOF) break;
        if (terminal != SANE_STATUS_GOOD) break;
    }
    if (interrupted) terminal = SANE_STATUS_CANCELLED;
    free(buf);
    if (fclose(raw) != 0 && terminal == SANE_STATUS_EOF) terminal = SANE_STATUS_IO_ERROR;
    diag("capture ended: total=%llu terminal=%d (%s)\n",
         (unsigned long long)total, (int)terminal, sane_strstatus(terminal));
    write_metadata(meta_path, device, &params, total, terminal);
    if (terminal == SANE_STATUS_EOF) {
        scan_active = false;
        st = raw_to_tiff(raw_path, tiff_path, &params) ? SANE_STATUS_GOOD : SANE_STATUS_IO_ERROR;
    } else {
        st = terminal;
    }

done:
    if (h && scan_active) {
        diag("abnormal active-scan termination: sane_cancel\n");
        sane_cancel(h);
        scan_active = false;
    }
    if (h) { diag("sane_close\n"); sane_close(h); }
    diag("sane_exit\n"); sane_exit();
    diag("client exit -> %d (%s)\n", (int)st, sane_strstatus(st));
    fclose(log_file);
    return st == SANE_STATUS_GOOD ? 0 : 1;
}
