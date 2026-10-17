# Phase7 import cleanup and storage profile

The suite and Clippy logs validate the checked-array warning index and failed
import root cleanup. The main profile is a separate instrumented build, with
terminal garbage collection; do not compare its wall time as a clean benchmark.

`ARENA_STORAGE` deduplicates frozen chunk allocations by identity across the
main program, analyzed deferred units and remaining snapshots. It counts table
capacity and chunk headers, not separately allocated entry payloads. `RUST_DATA`
counts live Datum nodes and Form node/buffer storage before and after terminal
collection. Reader preparation and environment conversion have separate timers.

The original profiler preparation scripts and instrumented checkout remain in
`/private/tmp/light-cljs-phase7/` and
`/private/tmp/light-cljs-phase7-storage-profile/`. These logs preserve the result;
the temporary profiler scripts are not a supported repository benchmark runner.
Use `bb benchmarks/penpot.bb OUTPUT-DIR` for clean timing runs.
