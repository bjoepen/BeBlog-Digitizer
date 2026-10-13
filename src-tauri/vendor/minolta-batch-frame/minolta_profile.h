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
bool minolta_parse_bounded_uint(const char *text, unsigned minimum, unsigned maximum,
                                unsigned *value);
bool minolta_validate_holder_polling(unsigned timeout_seconds, unsigned interval_seconds);
bool minolta_validate_cli_tail(const char *device, const char *prefix,
                               unsigned timeout_seconds, unsigned interval_seconds,
                               int first_unparsed_argument, int argument_count);
bool minolta_validate_rgb8_geometry(unsigned dpi, bool is_rgb, unsigned depth,
                                    int pixels_per_line, int lines, int bytes_per_line,
                                    MinoltaRgb8Geometry *geometry);

#endif
