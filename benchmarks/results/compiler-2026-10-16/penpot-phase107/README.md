# V8 runtime initialization guards — validated, still isolated

Based on phase102. The near-heap-limit callback is now installed before Context
creation, and the shared interrupt scheduler monitors the JavaScript runtime
initialization. Constructor failure uses the ordinary rooted-session cleanup;
the timeout, fuel or memory diagnostic survives engine termination. Scheduler
thread creation returns an owned error and can be retried by a later session.

The native Isolate/Context prelude remains synchronous. Elapsed time is checked
before JavaScript initialization; the conservative 16 MiB prototype startup
minimum remains. This does **not** close the backing-store accounting gap:
growable shared buffers are still unbounded by the existing heap guard. V8 is
not promoted, and no new complete-memory-limit claim is made.

The full suite passes **1,426 tests, zero failures, 106 ignored** at normal
concurrency, with strict Clippy, formatting and all three Node workflows.
Twenty focused engine tests include caught startup timeout/allocation failure,
already exhausted controls, syntax/initialization failures, scheduler allocation
failure, other live sessions and subsequent session recovery. Expected caught
host-panic diagnostics in the focused log are passing regressions.

Sequential cold builds, with no concurrent heavy job:

| Build | Wall | Peak RSS bytes |
| --- | ---: | ---: |
| light-cljs render | 3.77 s | 507,068,416 |
| light-cljs main none | 22.74 s | 956,006,400 |
| light-cljs main advanced | 28.93 s | 1,157,693,440 |
| Shadow release | 61.55 s | 4,810,047,488 |
| Shadow compile | 33.15 s | 2,592,915,456 |

All three light-cljs outputs are byte-identical to corrected production phase101
and pass explicit ES-module syntax checks. The 996-file port hash is unchanged.
This is syntax and suite validation, not a bundled/browser execution of Penpot.
The differing optimization/module scopes remain as documented in the main report.

Main is **31.4% faster** than fresh Shadow dev at **36.9% of its RSS**. This
matches the prior V8 performance range; the change closes initialization gaps
without a material speed penalty. The dramatic speed goal remains unmet.

`changes.patch` reproduces the implementation from phase102; commands, full
logs, hashes and timing files are retained. Production remains phase101.
