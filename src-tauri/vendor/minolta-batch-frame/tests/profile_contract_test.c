#include "../minolta_profile.h"

#include <assert.h>
int main(void) {
    unsigned dpi = 0;
    assert(minolta_parse_supported_dpi("600", &dpi) && dpi == 600U);
    assert(minolta_parse_supported_dpi("2820", &dpi) && dpi == 2820U);
    const char *bad_values[] = {"", "0", "2821", "600x", "-600", "+600", " 600",
                                "99999999999999999999"};
    for (unsigned index = 0; index < sizeof bad_values / sizeof bad_values[0]; ++index)
        assert(!minolta_parse_supported_dpi(bad_values[index], &dpi));

    MinoltaRgb8Geometry geometry = {0};
    assert(minolta_validate_rgb8_geometry(600, true, 8, 564, 850, 1692, &geometry));
    assert(geometry.raw_bytes == 1438200ULL);
    assert(!minolta_validate_rgb8_geometry(600, true, 8, 1200, 1800, 3600, &geometry));
    /* Synthetic geometry only; not evidence of a hardware 2,820-dpi scan. */
    assert(minolta_validate_rgb8_geometry(2820, true, 8, 1200, 1800, 3600, &geometry));
    assert(geometry.raw_bytes == 6480000ULL);
    assert(!minolta_validate_rgb8_geometry(2820, false, 8, 1200, 1800, 3600, &geometry));
    assert(!minolta_validate_rgb8_geometry(2820, true, 16, 1200, 1800, 7200, &geometry));
    assert(!minolta_validate_rgb8_geometry(2820, true, 8, 1200, 1800, 3599, &geometry));
    assert(!minolta_validate_rgb8_geometry(2820, true, 8, 100000, 100000, 300000, &geometry));
    assert(!minolta_validate_rgb8_geometry(2820, true, 8, 2147483647, 2147483647,
                                           2147483647, &geometry));
    return 0;
}
