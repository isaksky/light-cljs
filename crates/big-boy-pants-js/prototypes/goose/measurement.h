/* Process high-water RSS; Darwin reports bytes, Linux reports KiB. */
#include <sys/resource.h>
static gs_i64 goose_measure_peak_rss(void) {
    struct rusage usage;
    return getrusage(RUSAGE_SELF, &usage) == 0 ? (gs_i64)usage.ru_maxrss : 0;
}
