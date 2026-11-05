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

/* Read-only, compiler-specific diagnostics for the separate storage run.
   Neither function changes allocator metadata or accesses discarded elements. */
static gs_i64 goose_measure_address(r_u8 bytes) {
    return (gs_i64)(uintptr_t)bytes;
}
static gs_i64 goose_measure_regions(void) { return (gs_i64)gs_nstks; }

/* Current resident memory in bytes, for the trim_memory() measurement. */
#ifdef __APPLE__
#include <mach/mach.h>
static gs_i64 goose_measure_resident(void) {
    mach_task_basic_info_data_t info;
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    return task_info(mach_task_self(), MACH_TASK_BASIC_INFO, (task_info_t)&info, &count) == KERN_SUCCESS ? (gs_i64)info.resident_size : 0;
}
#else
#include <stdio.h>
#include <unistd.h>
static gs_i64 goose_measure_resident(void) {
    long pages = 0, resident = 0;
    FILE *f = fopen("/proc/self/statm", "r");
    if (!f) return 0;
    int ok = fscanf(f, "%ld %ld", &pages, &resident) == 2;
    fclose(f);
    return ok ? (gs_i64)resident * (gs_i64)sysconf(_SC_PAGESIZE) : 0;
}
#endif
