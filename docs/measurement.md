# Measurement and command verification

These measurements describe this standalone runtime only. There is no matched
comparison with QuickJS, V8 or the unavailable compiler. The source-driven
architectural proofs and their limitations are mapped in
[architecture.md](architecture.md#requirement-map).

## Reproduce

Inside the pinned `nix develop` environment, use `CARGO_BUILD_JOBS=3` throughout.
Test commands use `--test-threads=3`. Release measurements:

```sh
cargo run --release --example bench
cargo run --release --example bench -- --quick
cargo run --release --example bench -- memory
cargo run --release --example bench -- work --workers 4 --rounds 9
cargo run --release --example bench -- strings --rounds 3
cargo test --test measurement -- --test-threads=3
```

The full invocation runs every row group; quick uses smaller sizes. The
`memory` and `strings` selectors isolate those groups. The work selector above
uses four executor workers and nine measured rounds; tests still use three
test threads. The benchmark owns its default target output only.

## Method and limits of the measurements

`examples/bench/main.rs` handles timing, a counting allocator and reports;
`examples/bench/workloads.rs` supplies workloads shared with
`tests/measurement.rs`. JavaScript in `examples/bench/` and `examples/e2e/work.js`
is synthetic fixture code, not light-cljs output. No benchmark dependency or
C-engine wrapper is used. QuickJS `tests/microbench.js` at the attributed
revision informed only the named-row/empty-loop reporting style; no workload
or score is copied from it.

Timing rows use one discarded warmup, then five measured rounds (three in quick
mode), reporting median and [min..max]. Each round uses a fresh Runtime, with
setup outside the interval: compilation, input building, task spawning except
in startup rows, and pre-completion of ready cells. Results are checked against
Rust-computed values after timing. Rows pairing a JS intrinsic loop with an
arithmetic baseline report the difference as an estimate, not isolated opcode
cost. Most rows run on one embedder thread; `work` uses the reference executor.

The counting allocator is enabled only for memory/count passes; it counts
requested bytes rather than allocator overhead. Memory rows run once. Shared
input/code are measured once; per-task spawn and park bytes include task state,
root registration, waiter entries, private heap objects and table growth.
`kept_B/t` is capacity retained after task drop and collection. Host `Vec<Task>`
storage is excluded from per-task figures. `HeapUsage` snapshots count live
objects/table capacity and shallow owned bytes, excluding trie bodies, code,
atom cells and task/native state. `TaskStats` gives shallow saved-state estimates,
not total retained memory. RSS includes previous row groups, allocator caching
and everything else in the process. A memory-only run avoids earlier update
rows dominating high-water RSS.

Statistics are read-only and not free: heap_usage walks object slots under the
object lock; Task::stats briefly locks saved task state and returns None while
a run holds it. The report records CPU/profile/parallelism and start/end load.
Variation on a shared machine prevents close performance comparisons. Counting
allocation changes timing, so ordinary timing rows leave it off.

Not isolated here: cross-thread wake latency, heavily contended heap/cell/atom
operations, collection pauses under load, needAll retained memory, actual compiler
startup/workloads or comparisons with another engine. Explicit GC means persistent
update garbage remains allocated until collection. First rope flattening is
synchronous; the following unit reads reuse the cache. Rope release is outside
the timing interval. Runtime resource/policy caps are in
[grammar.md](grammar.md#resource-and-policy-limits).

## Current representative run

Recorded 2026-10-25 UTC at code baseline `72201c8b`, before this documentation
rewrite; no runtime code changed. Linux 6.18.57 x86_64, AMD Ryzen 7 9850X3D,
Rust 1.95.0, optimized release build. The harness reports 14 available logical
CPUs; this pass had no peer workers and capped Cargo jobs at three. Full mode,
one warmup plus five measured rounds, two executor workers. Load averages at
start/end: 2.32/1.68 (one-minute values). Full invocation including its cold
release compilation took 66 seconds. Historical numbers from earlier layouts
are superseded by this run, not treated as a matched performance comparison.

| Row | Median [min..max] | Unit / workload |
| --- | ---: | --- |
| startup.spawn | 124.3 [124.2..125.1] | ns/task, 20,000 tasks |
| startup.first_run_to_complete | 232.0 [230.9..232.3] | ns/task |
| reads.js_need_loop | 472.9 [472.7..474.8] | ns/iteration, 1,000,000 reads |
| reads.js_baseline_loop | 325.8 [323.2..326.7] | ns/iteration |
| reads.js_need_minus_baseline | 146.9 [146.2..151.6] | ns/read estimate |
| reads.rust_runguard_need | 90.6 [90.6..91.2] | ns/read |
| park.full_cycle | 924.7 [915.5..925.2] | ns/cycle, 100,000 cycles |
| park.complete_and_wake | 122.9 [122.6..123.1] | ns/cycle |
| persistent.vec_nth_minus_baseline | 68.1 [67.8..73.4] / 71.9 [67.9..74.0] | ns/op at 32 / 32,768 elements |
| persistent.map_get_minus_baseline | 83.9 [82.6..87.1] / 132.8 [127.9..139.1] | ns/op at 32 / 32,768 entries |
| persistent.vec_conj_loop | 586.3 [585.0..589.1] | ns/iteration, 100,000 updates |
| persistent.map_assoc_loop | 1260.0 [1245.3..1273.9] | ns/iteration, 100,000 updates |
| work.alone | 1188.7 [1099.5..1207.1] | us/task, 64 tasks |
| work.while_consumers_parked | 1289.6 [1225.3..1470.7] | us/task, plus 1000 parked consumers |
| work.publish_to_all_resumed | 1352.9 [1314.0..1977.3] | ns/consumer, including executor queueing |
| strings.build, append 10k / 100k | 1465.1 [1435.4..1489.7] / 1426.6 [1417.3..1438.8] | ns/unit |
| strings.build, prepend 10k / 100k | 1458.2 [1445.3..1480.1] / 1411.8 [1403.0..1440.1] | ns/unit |
| strings.flatten, append 100k | 5.0 [4.4..5.3] | ns/unit, first contiguous read |
| overhead.startup_counting_off / on | 395.8 [395.1..396.3] / 437.1 [435.9..437.7] | ns/task; instrumentation changes cost |

Every parked consumer remained parked until useful work completed. Timing alone
does not establish equal throughput or scaling; the asserted still-parked count
establishes that waiting did not occupy those workers.

Shared input stayed **3001 objects / 1,428,020 requested bytes** from 1 to 10,000
consumers. Code was shared once: 2226 bytes for consumer.js, 3080 for
consumer_fold.js. Each consumer added one live heap object (builder or callback),
every returned input subtree kept identity, and only the 3001 input objects
remained after task drop and collection. Selected memory rows:

| Fixture / consumers | Spawn B/task | Park B/task | Shallow VM B/task | Live heap objects | Retained capacity B/task |
| --- | ---: | ---: | ---: | ---: | ---: |
| consumer / 1 | 1192.0 | 1252.0 | 912 | 3002 | 444.0 |
| consumer / 10,000 | 1154.2 | 712.0 | 912 | 13001 | 85.2 |
| fold / 1 | 1216.0 | 2644.0 | 1304 | 3002 | 444.0 |
| fold / 10,000 | 1178.2 | 2104.0 | 1304 | 13001 | 52.5 |

The update counting pass retained 1494.0 B/conj and 5732.4 B/assoc until explicit
collection, which released 717,396,504 requested bytes after both workloads.
Full-run RSS high-water reached 1,030,120 kB, dominated by those preceding updates;
it is not a suspended-task memory estimate.

## Verification run

The command catalogue below records the old README/docs command inventory and
its results in this worktree. Wrappers were normalized to the pinned development
shell and `CARGO_BUILD_JOBS=3`; each test invocation includes
`-- --test-threads=3` (or appends that flag to its existing harness options).
Placeholder script paths (`path/to/script.js`, `file.js`, `<file>`) were replaced
with `examples/demo.js`; they were notation, not runnable files. The revised
README uses the concrete path. Duplicate invocations caused by flag/wrapper
variants were still run. Build/test/example/measurement commands no longer live
in hundreds of feature paragraphs; the maintained entry points are README and
this document.

One stale test filter, `frontend::tests::delete_of_identifiers_is_rejected`,
exited successfully while selecting zero tests. It is retired; the correct
filter is `frontend::tests::delete_of_identifiers_follows_strictness` (one passing
test). An inventory run during the architecture rewrite saw the temporarily
missing Root inventory section and failed; after the table was restored its
16 tests passed. This was an editing-order documentation failure, not a runtime
defect. No assertions or fixture bodies were changed.
