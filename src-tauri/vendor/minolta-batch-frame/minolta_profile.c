#include "minolta_profile.h"

#include <errno.h>
#include <stdlib.h>

#define MINOLTA_MAX_RAW_BYTES (512ULL * 1024ULL * 1024ULL)
#define TIFF_CLASSIC_MAX_BYTES (0xffffffffULL - 4096ULL)

bool minolta_parse_bounded_uint(const char *text, unsigned minimum, unsigned maximum,
                                unsigned *value) {
    if (!text || !*text || !value || minimum > maximum) return false;
    for (const char *cursor = text; *cursor; ++cursor) {
        if (*cursor < '0' || *cursor > '9') return false;
    }
    errno = 0;
    char *end = NULL;
    unsigned long parsed = strtoul(text, &end, 10);
    if (errno == ERANGE || end == text || *end != '\0' || parsed < minimum ||
        parsed > maximum || parsed > UINT32_MAX) return false;
    *value = (unsigned)parsed;
    return true;
}

bool minolta_parse_supported_dpi(const char *text, unsigned *dpi) {
    if (!dpi) return false;
    return minolta_parse_bounded_uint(text, 600U, 2820U, dpi) &&
           (*dpi == 600U || *dpi == 2820U);
}

bool minolta_validate_holder_polling(unsigned timeout_seconds, unsigned interval_seconds) {
    return timeout_seconds >= 1U && timeout_seconds <= 3600U &&
           interval_seconds >= 1U && interval_seconds <= 60U &&
           interval_seconds <= timeout_seconds;
}

bool minolta_validate_cli_tail(const char *device, const char *prefix,
                               unsigned timeout_seconds, unsigned interval_seconds,
                               int first_unparsed_argument, int argument_count) {
    return device && *device && prefix && *prefix &&
           minolta_validate_holder_polling(timeout_seconds, interval_seconds) &&
           first_unparsed_argument == argument_count;
}

bool minolta_validate_rgb8_geometry(unsigned dpi, bool is_rgb, unsigned depth,
                                    int pixels_per_line, int lines, int bytes_per_line,
                                    MinoltaRgb8Geometry *geometry) {
    if ((dpi != 600U && dpi != 2820U) || !is_rgb || depth != 8U ||
        pixels_per_line <= 0 || lines <= 0 || bytes_per_line <= 0 || !geometry) {
        return false;
    }
    uint64_t row_bytes = (uint64_t)(unsigned)pixels_per_line * 3ULL;
    if (row_bytes > UINT32_MAX || row_bytes != (uint64_t)(unsigned)bytes_per_line) {
        return false;
    }
    if (dpi == 600U &&
        (pixels_per_line != 564 || lines != 850 || bytes_per_line != 1692)) {
        return false;
    }
    uint64_t total = row_bytes * (uint64_t)(unsigned)lines;
    if (total == 0 || total > MINOLTA_MAX_RAW_BYTES || total > TIFF_CLASSIC_MAX_BYTES) {
        return false;
    }
    geometry->pixels_per_line = (uint32_t)pixels_per_line;
    geometry->lines = (uint32_t)lines;
    geometry->bytes_per_line = (uint32_t)bytes_per_line;
    geometry->raw_bytes = total;
    return true;
}
