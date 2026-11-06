/* Exactly `count` bytes of stdin appended to a Goose byte builder: the
   payload of a compiler frame (light-cljs-816). It reads through the same
   stdio stream as read_line, so a frame's header line and its payload come
   from one buffer. False when stdin ends first: a truncated frame. */
#include <stdio.h>
static uint8_t goose_pipe_read_exact(gs_rref out, gs_i64 count) {
    uint8_t buf[65536];
    while (count > 0) {
        size_t want = count < (gs_i64)sizeof buf ? (size_t)count : sizeof buf;
        size_t n = fread(buf, 1, want, stdin);
        if (n == 0) return 0;
        gs_bld_append(out, buf, (int64_t)n);
        count -= (gs_i64)n;
    }
    return 1;
}
