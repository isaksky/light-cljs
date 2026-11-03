/* Process high-water RSS; Darwin reports bytes, Linux reports KiB. */
#include <sys/resource.h>
static gs_i64 goose_measure_peak_rss(void) {
    struct rusage usage;
    return getrusage(RUSAGE_SELF, &usage) == 0 ? (gs_i64)usage.ru_maxrss : 0;
}

/* Keep a flat copy observable to the optimizer without a per-byte checksum.
   This barrier may access every byte; it performs no allocation or copying. */
static __attribute__((noinline)) gs_i64
goose_measure_observe(r_u8 bytes, gs_i64 count) {
    __asm__ __volatile__("" : : "r"(bytes), "r"(count) : "memory");
    return count + bytes[count - 1];
}
