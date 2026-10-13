#ifndef BEBLOG_MINOLTA_PROFILE_H
#define BEBLOG_MINOLTA_PROFILE_H

#include <stdbool.h>
#include <stdint.h>

typedef struct {
    uint32_t pixels_per_line;
    uint32_t lines;
    uint32_t bytes_per_line;
    uint64_t raw_bytes;
} MinoltaRgb8Geometry;

bool minolta_parse_supported_dpi(const char *text, unsigned *dpi);
bool minolta_validate_rgb8_geometry(unsigned dpi, bool is_rgb, unsigned depth,
                                    int pixels_per_line, int lines, int bytes_per_line,
                                    MinoltaRgb8Geometry *geometry);

#endif
