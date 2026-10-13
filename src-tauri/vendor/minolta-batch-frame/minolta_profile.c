#include "minolta_profile.h"

#include <errno.h>
#include <stdlib.h>

#define MINOLTA_MAX_RAW_BYTES (512ULL * 1024ULL * 1024ULL)
#define TIFF_CLASSIC_MAX_BYTES (0xffffffffULL - 4096ULL)

bool minolta_parse_supported_dpi(const char *text, unsigned *dpi) {
    if (!text || !*text || !dpi) return false;
    for (const char *cursor = text; *cursor; ++cursor) {
        if (*cursor < '0' || *cursor > '9') return false;
    }
    errno = 0;
    char *end = NULL;
    unsigned long value = strtoul(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' || value > UINT32_MAX) return false;
    if (value != 600UL && value != 2820UL) return false;
    *dpi = (unsigned)value;
    return true;
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
