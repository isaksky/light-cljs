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

/* The files the runner has created in its session directory, across every
   thread of the process (light-cljs-822): create_file counts each one before
   it writes it, and the runner reports the total as `FILES 0 n` before
   `CLOSED 1`, so a file created and removed again is still counted. */
#include <stdatomic.h>
static _Atomic gs_i64 goose_pipe_files = 0;
static gs_i64 goose_pipe_count_file(void) { return atomic_fetch_add(&goose_pipe_files, 1) + 1; }
static gs_i64 goose_pipe_files_created(void) { return atomic_load(&goose_pipe_files); }
