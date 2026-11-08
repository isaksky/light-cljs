/* The run-time compiler boundary (light-cljs-856; docs/javascript-coverage.md,
   decision 1, "Run-time compilation"): the C side of runtime_compile.goose's
   `extern fn compile_runtime`. It opens bbp's Rust frontend, the shared
   library `cargo build --release -p big-boy-pants-js --example
   goose_frontend` builds, on the first call in the process and never before,
   so a program that never compiles at run time neither loads nor finds it.
   The library is $BBP_FRONTEND_LIBRARY when that is set, and otherwise
   target/release/examples/ of the repository this header is in (the default
   Cargo target directory). It compiles on a helper thread of its own
   (export_runtime) and returns bytes; it never runs JavaScript.

   Status: 0 `out` holds a GSP image; 1 a compile error, `out` its UTF-8
   message and *line/*column its position; 2 the frontend refused (`out` the
   reason); 3 the library or its symbols could not be loaded (`out` why).

   `goose run` (in-process TinyCC, the `--debug` runs without `build`) has
   neither dlfcn.h nor the C library's symbols, so there every call is
   status 3: run-time compilation needs a program `goose build` made. */
#if defined(__TINYC__)
static gs_i32 goose_runtime_compile(gs_rref out, r_u16 units, gs_i64 count, gs_i32 mode,
                                    uint8_t strict, gs_i64 parameter_units, r_i64 line,
                                    r_i64 column) {
    static const char why[] = "goose run cannot load the run-time compiler; use goose build";
    (void)units; (void)count; (void)mode; (void)strict; (void)parameter_units;
    gs_bld_append(out, why, (int64_t)(sizeof why - 1));
    *line = 0;
    *column = 0;
    return 3;
}
#else
#include <dlfcn.h>
#include <pthread.h>
#include <stdlib.h>
#include <string.h>

typedef int32_t (*goose_runtime_compile_fn)(const uint16_t *, int64_t, int32_t, int32_t, int64_t,
                                            const uint8_t *, int64_t, uint8_t **, int64_t *,
                                            int64_t *, int64_t *);
typedef void (*goose_runtime_free_fn)(uint8_t *, int64_t);
static pthread_once_t goose_runtime_once = PTHREAD_ONCE_INIT;
static goose_runtime_compile_fn goose_runtime_compile_entry;
static goose_runtime_free_fn goose_runtime_free_entry;
static char goose_runtime_error[1024];

static void goose_runtime_open(void) {
#ifdef __APPLE__
    const char *leaf = "libgoose_frontend.dylib";
#else
    const char *leaf = "libgoose_frontend.so";
#endif
    char path[4096];
    const char *chosen = getenv("BBP_FRONTEND_LIBRARY");
    if (!chosen || !*chosen) {
        /* This header is crates/big-boy-pants-js/src/runtime_compiler.h. */
        const char *file = __FILE__;
        const char *slash = strrchr(file, '/');
        size_t dir = slash ? (size_t)(slash - file) : 0;
        int n = snprintf(path, sizeof path, "%.*s/../../../target/release/examples/%s", (int)dir,
                         file, leaf);
        if (n <= 0 || (size_t)n >= sizeof path) {
            snprintf(goose_runtime_error, sizeof goose_runtime_error,
                     "the run-time compiler's default path does not fit");
            return;
        }
        chosen = path;
    }
    void *h = dlopen(chosen, RTLD_NOW | RTLD_LOCAL);
    if (!h) {
        const char *why = dlerror();
        snprintf(goose_runtime_error, sizeof goose_runtime_error,
                 "cannot load the run-time compiler %s (set BBP_FRONTEND_LIBRARY): %s", chosen,
                 why ? why : "unknown error");
        return;
    }
    goose_runtime_compile_fn compile = (goose_runtime_compile_fn)dlsym(h, "bbp_goose_compile");
    goose_runtime_free_fn release = (goose_runtime_free_fn)dlsym(h, "bbp_goose_free");
    if (!compile || !release) {
        snprintf(goose_runtime_error, sizeof goose_runtime_error,
                 "the run-time compiler %s lacks bbp_goose_compile or bbp_goose_free", chosen);
        return;
    }
    goose_runtime_free_entry = release;
    goose_runtime_compile_entry = compile;
}

static gs_i32 goose_runtime_compile(gs_rref out, r_u16 units, gs_i64 count, gs_i32 mode,
                                    uint8_t strict, gs_i64 parameter_units, r_i64 line,
                                    r_i64 column) {
    /* Every worker thread may compile; the library opens once. */
    pthread_once(&goose_runtime_once, goose_runtime_open);
    if (!goose_runtime_compile_entry) {
        gs_bld_append(out, goose_runtime_error, (int64_t)strlen(goose_runtime_error));
        return 3;
    }
    uint8_t *data = NULL;
    int64_t length = 0, at_line = 0, at_column = 0;
    int32_t status = goose_runtime_compile_entry((const uint16_t *)units, count, mode, strict,
                                                 parameter_units, NULL, 0, &data, &length,
                                                 &at_line, &at_column);
    if (data && length > 0) gs_bld_append(out, data, length);
    goose_runtime_free_entry(data, length);
    *line = at_line;
    *column = at_column;
    return status;
}
#endif
