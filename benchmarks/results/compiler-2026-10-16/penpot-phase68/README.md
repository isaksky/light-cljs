# Compiled namespace registration cache — isolated V8 candidate

The emitter supplies exact namespace chunk boundaries. V8 caches the compiled
registration function for each namespace's latest source, with limits of 1,024
entries and 4 MiB of source. Every registration still executes in order, making
fresh namespace maps/sets. Changed source, old snapshots, namespace mutations
and reentrant compilation therefore retain the existing behavior. Replaced
global `eval` selects the original evaluation path. Other evaluators use the
default full-source implementation.

Penpot has **131,239 cache hits / 1,292 misses**. Compiled namespace source falls
from 102.9 MB to **1,050,924 bytes**. The final cache contains 851 entries and
717,833 source bytes. This diagnostic run, including terminal GC/statistics,
takes **24.62 s / 1,172,520,960 bytes peak RSS**; full output matches phase42.

All six focused V8 tests pass, including three new tests for repeated execution
after mutation, old source views, eviction, eval replacement, reentry and stopping
at the first exception. No standalone full suite was run for this phase. The
benchmark preceded the test-only additions. The cache is unpromoted; the known
V8 resource gaps and baseline main ESM defect remain.

Patch relative to phase58, commands, tests, diagnostics and hashes are archived.
Frozen binary and complete output remain under `/private/tmp/light-cljs-phase68`.
