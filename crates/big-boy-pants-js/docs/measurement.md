# Measurement and command verification

These measurements describe the engine package. Compiler-shaped kernels below
include an optional Node jitless reference; historical standalone rows do not
compare against other engines or the compiler. The source-driven
architectural proofs and their limitations are mapped in
[architecture.md](architecture.md#requirement-map).

## Reproduce

Run these commands from `crates/big-boy-pants-js/` in the light-cljs checkout.
Cargo now uses the root workspace profiles, lockfile and `target/` directory.
The results below predate that import and used the standalone crate's profiles;
they are historical measurements, not a baseline for the workspace build.

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

## Compiler-shaped kernel baseline (2026-10-25)

Run from this crate directory (or add `-p big-boy-pants-js` from the repository
root):

```sh
cargo run --locked --release --example bench -- scaling
cargo run --locked --release --example bench -- kernels
bb scripts/kernels.bb
bb scripts/kernels.bb kernels --quick --rounds 3
```

`kernels` selects single-thread ns/iteration; `scaling` selects 1, 2, 4 and 8
registered OS threads. Both select the same five sources. `--quick` reduces
sizes, and `--rounds N` changes the number of measured rounds. The default is
one discarded warmup and five measured rounds, each with a fresh `Runtime`.
Compilation, Runtime construction, thread creation/registration, and numeric
argument rooting happen before the timer. The start barrier releases all
workers together. Each worker spawns and runs four tasks to completion using
one shared executable and Runtime. Spawn/run, result assertions, task/result
drop and worker joins are timed. Runtime destruction and collection are not;
there is no collection between tasks, matching the strategy's original probe.
The result is aggregate iterations/s and wall ns per aggregate iteration;
speedup is the one-thread median ns divided by the row median ns. Spreads are
min..max of measured rounds, not confidence intervals. Work per thread stays
constant, so total work and retained garbage grow with thread count.

The arithmetic loop touches only VM locals (task startup still allocates).
`allocate` is the strategy's exact cons/map/count body, with list length 1000;
one iteration builds and maps one list, using constructor objects, a captured
closure and recursion. `properties` repeatedly reads/writes two object shapes.
Their full task sizes are 400,000 loop bodies, 40 list pairs, and 40,000 loop
bodies respectively. The `cljs_map` and `cljs_protocol` rows each repeat the
checked emitted `tests/fixtures/light-cljs/{map,protocol}.js` program 40 times
per task. Each iteration performs the corpus's 16 assoc/reduce steps or 16
protocol calls/Box allocations. Only the runtime declaration is hoisted once
per task; emitted expressions, namespace setup, protocol registration and
expected totals remain unchanged. Runtime factory initialization is included
once per task. These are real-output kernels, not warmed macro sessions;
their iteration units differ from the synthetic loops and from one another.
No mutable JavaScript runtime object is shared between tasks.

The Babashka script works from any directory and runs the release bench. If
`node` is on PATH it asks the bench for `--node-kernels`, which emits the exact
same source, sizes and expected results, then runs it under `node --jitless`.
It prints a reference column and engine/Node ratio for the one-thread rows.
Node uses the same warmup, measured rounds and four calls per round; parsing
and process startup are outside its timer. Node's GC policy and lack of engine
task bookkeeping differ. Node keeps one process across rounds, so built-in
caches may also differ. This is an end-to-end kernel reference rather
than an isolated opcode comparison. Node is optional and never invoked by
Rust tests. Small correctness instances run all five kernels with 1/2/4/8
threads in `tests/measurement.rs`; neither tests nor scripts impose a speed
threshold.

### Recorded Mac run

Measured on the 24-CPU Apple M3 Ultra (Virtual), 32 GiB RAM, macOS 15.7.3
(24G419), Homebrew Rust 1.99.1 (`58cf8f9c3`), Node v26.12.1, Babashka
v1.12.218. The workspace release profile uses thin LTO and one codegen unit;
no `RUSTFLAGS` or `RUST_MIN_STACK` override was set. Command, from this crate:
`CARGO_BUILD_JOBS=3 bb scripts/kernels.bb`. This ran the release `bench -- scaling`
with its full sizes, four tasks per thread, one warmup and five measured rounds,
then the Node reference. The tracked checkout was clean. Built/measured commit:
`bca99ec4e783166ce1c5975538d1a61591df8597`; the same benchmark/runtime/build inputs
are preserved at `37738cf5c076f5ca661ba13ffa4e337aa9468deb` after rebasing onto
`3a6de453` (only an unrelated promise test's scratch-directory setup changed).

Other orchestration builds/tests were stopped for the entire timing window.
Host load averages (1/5/15 minute) were `4.82 3.97 3.22` before and
`7.10 6.80 5.67` after the engine run; the latter includes the benchmark's own
workers. Background host activity was not otherwise controlled.

| Kernel | Threads | ns/iteration median [min..max] | Iterations/s | Speedup |
|---|---:|---:|---:|---:|
| arithmetic | 1 | 329.5 [322.4..330.1] | 3035320.9 | 1.000x |
| arithmetic | 2 | 167.3 [167.0..171.9] | 5978547.3 | 1.970x |
| arithmetic | 4 | 86.5 [85.3..87.6] | 11563903.5 | 3.810x |
| arithmetic | 8 | 43.3 [43.2..46.0] | 23076801.7 | 7.603x |
| allocate | 1 | 13554648.2 [13315575.0..13648202.1] | 73.8 | 1.000x |
| allocate | 2 | 11112343.8 [10500254.0..18082029.7] | 90.0 | 1.220x |
| allocate | 4 | 26305818.9 [26176690.6..26493554.2] | 38.0 | 0.515x |
| allocate | 8 | 31815732.6 [31570174.3..32477078.9] | 31.4 | 0.426x |
| properties | 1 | 6107.8 [6104.6..6173.5] | 163725.1 | 1.000x |
| properties | 2 | 3794.0 [3760.6..3923.4] | 263574.6 | 1.610x |
| properties | 4 | 4258.4 [3980.0..4408.3] | 234832.5 | 1.434x |
| properties | 8 | 10486.6 [10357.4..10627.4] | 95359.4 | 0.582x |
| cljs_map | 1 | 16264334.4 [15961063.0..16376408.9] | 61.5 | 1.000x |
| cljs_map | 2 | 15767915.5 [14745397.8..16613702.5] | 63.4 | 1.031x |
| cljs_map | 4 | 40901901.4 [40022752.5..41196247.4] | 24.4 | 0.398x |
| cljs_map | 8 | 45031670.5 [44053418.6..45313605.2] | 22.2 | 0.361x |
| cljs_protocol | 1 | 1824261.5 [1815839.1..1896904.9] | 548.2 | 1.000x |
| cljs_protocol | 2 | 1708634.2 [1679633.6..1783499.6] | 585.3 | 1.068x |
| cljs_protocol | 4 | 4560010.2 [4436908.8..4633556.2] | 219.3 | 0.400x |
| cljs_protocol | 8 | 5020414.7 [4972272.2..5059423.8] | 199.2 | 0.363x |

| Kernel | Node jitless ns/iteration median [min..max] | Engine / Node median |
|---|---:|---:|
| arithmetic | 9.4 [9.3..9.5] | 35.05x |
| allocate | 131736.7 [131169.8..133569.8] | 102.89x |
| properties | 34.9 [34.3..35.8] | 175.01x |
| cljs_map | 340387.5 [331689.1..341481.5] | 47.78x |
| cljs_protocol | 37695.6 [35213.0..42745.3] | 48.39x |

The allocation kernel's **0.426x at eight threads reproduces the roughly 0.4x
collapse** in the strategy (its earlier single-run probe reported 0.37x).
This uses the same 40 rounds of 1000-element lists and four tasks per thread;
barrier-based timing now excludes thread creation, and the table reports
medians/spreads rather than one sample. Arithmetic reaches 7.603x, while both
real-output kernels fall to about 0.36x. These measurements establish a
baseline, not an achieved Phase 0 performance gate. The current single-thread
ratios are recorded as measured; the strategy's coarse 30–45x process-wall-time
estimate is not a bound on these rows, especially the property/allocate loops.

Validation: all 10 `--test measurement` tests passed after the stack-safety
integration, as did engine formatting and Clippy with only the three existing
Rust 1.99 diagnostic categories allowed. Strict Clippy still reports the nine
existing diagnostics described below. The full engine suite was not repeated
for this benchmark-only change. Quick scaling plus Node and single-thread
script modes also completed successfully. The final rebase changed only an
unrelated test setup; benchmark and measurement-test inputs were unchanged.

## Method and limits of the measurements

`examples/bench/main.rs` handles timing, a counting allocator and reports;
`examples/bench/workloads.rs` supplies workloads shared with
`tests/measurement.rs`. JavaScript in `examples/bench/` and `examples/e2e/work.js`
is synthetic fixture code, not light-cljs output. No benchmark dependency or
C-engine wrapper is used. QuickJS `tests/microbench.js` at the attributed
revision informed only the named-row/empty-loop reporting style; no workload
or score is copied from it.

The separate `tests/light_cljs.rs` compatibility gate now runs real emitted
programs and the current compiler runtime factory. Its checked map, vector and
protocol loops are available in `tests/fixtures/light-cljs/` with provenance and
expected results. They have not been substituted into the historical benchmark
rows below, and passing them is not a timing or scaling measurement.

The compatibility pass on macOS with Rust 1.99.1 ran 739 tests successfully
across `light_cljs`, `map_iterables`, `object_statics`, `quickjs_collections`,
`sets`, `js_source`, `exceptions`, `exception_waits` and `need`, using
`cargo test --locked -p big-boy-pants-js` with those `--test` arguments and
`-- --test-threads=3`. The full `--all-targets` command aborted in
`frontend::tests::deep_nesting_is_rejected_without_overflowing_the_stack` with
a native stack overflow. Running that exact test alone with `--lib`,
`-- --exact --test-threads=3` also aborted on untouched `074706d4` with the same
toolchain. This is a pre-existing stack-safety limitation, not a passing suite.

Strict `cargo clippy --locked -p big-boy-pants-js --all-targets -- -D warnings`
reported the same nine diagnostics on that baseline and this pass: deprecated
`Atomic::fetch_update`, `clippy::chunks_exact_to_as_chunks`, and
`clippy::byte_char_slices` in existing symbol, intrinsic and Intl code. Allowing
only those three categories after `-D warnings` passed; no lint suppression was
added to the source. The package's `cargo fmt -- --check` and doctests passed.

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

The pre-consolidation inventory contained 368 normalized command spellings.
The 365 invocations below were executed before the finish checks; the other
three (full suite, fmt, clippy) are recorded separately. The clj-quickjs-30 corpus
passed **856 tests in 17 binaries**, with no failures. Its exact command is row
63 and is maintained in README. No runtime code defect was found by this sweep.

Additional checks, also with CARGO_BUILD_JOBS=3 in the pinned shell:

| Exact command | Result |
| --- | --- |
| `cargo test --lib heap::inventory:: -- --test-threads=3` | 16 passed after root table restoration |
| `cargo test --lib frontend::tests::grammar_doc_examples -- --exact --test-threads=3` | 1 passed, covering all 351 retained fences |
| `cargo test --lib frontend::tests::delete_of_identifiers_follows_strictness -- --exact --test-threads=3` | 1 passed; replacement for the retired zero-match filter |
| `cargo check --all-targets` | passed |
| `cargo fmt --all -- --check` | passed |
| `cargo clippy --all-targets -- -D warnings` | passed |

Both demo file invocations completed with 89; resumable_counter made five
host effects over 26 runs. All four e2e variants and all five benchmark
invocations completed with their internal checks. The exact exit/results below
include redundant wrapper/flag variants intentionally retained for the audit.
Elapsed seconds include build/lock wait and are not benchmark timings.

<details>
<summary>Complete command inventory and observed results</summary>

Every row uses CARGO_BUILD_JOBS=3 inside nix develop. Test flags are shown
exactly; a successful zero-match retired filter is labelled rather than counted
as proof. Full-suite/fmt/clippy results are outside this historical catalogue.

| # | Executed command | Result | Elapsed s |
| ---: | --- | --- | ---: |
| 1 | `cargo build --all-targets` | passed | 14 |
| 2 | `cargo test --doc -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 3 | `cargo run` | passed | 0 |
| 4 | `cargo run -- examples/demo.js` | passed | 0 |
| 5 | `cargo run --example resumable_counter` | passed | 0 |
| 6 | `cargo test --lib resume_tests -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 7 | `cargo test --test resumable -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 8 | `cargo test --test gc -- --test-threads=3` | 51 passed, 0 failed | 0 |
| 9 | `cargo test --lib heap:: -- --test-threads=3` | 49 passed, 0 failed | 0 |
| 10 | `cargo test --test collections -- --test-threads=3` | 9 passed, 0 failed | 0 |
| 11 | `cargo test --lib collections:: -- --test-threads=3` | 14 passed, 0 failed | 3 |
| 12 | `cargo test --test collection_intrinsics -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 13 | `cargo test --lib vm::intrinsics:: -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 14 | `cargo test --test publication -- --test-threads=3` | 30 passed, 0 failed | 0 |
| 15 | `cargo test --lib value::share:: -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 16 | `cargo test --test dependency -- --test-threads=3` | 14 passed, 0 failed | 0 |
| 17 | `cargo test --lib dep:: -- --test-threads=3` | 11 passed, 0 failed | 0 |
| 18 | `cargo test --test js_source -- --test-threads=3` | 579 passed, 0 failed | 71 |
| 19 | `cargo test --test js_functions -- --test-threads=3` | 61 passed, 0 failed | 2 |
| 20 | `cargo test --lib frontend:: -- --test-threads=3` | 103 passed, 0 failed | 11 |
| 21 | `cargo test --test wakeup -- --test-threads=3` | 4 passed, 0 failed | 1 |
| 22 | `cargo test --lib task:: -- --test-threads=3` | 98 passed, 0 failed | 3 |
| 23 | `cargo test --test sim_host -- --test-threads=3` | 31 passed, 0 failed | 0 |
| 24 | `cargo test --lib dep::graph:: -- --test-threads=3` | 7 passed, 0 failed | 0 |
| 25 | `cargo test --test atoms -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 26 | `cargo test --lib atom:: -- --test-threads=3` | 7 passed, 0 failed | 0 |
| 27 | `cargo test --test cancellation -- --test-threads=3` | 3 passed, 0 failed | 1 |
| 28 | `cargo test --lib task::cancel_tests -- --test-threads=3` | 15 passed, 0 failed | 0 |
| 29 | `cargo test --test safepoints -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 30 | `cargo test --lib heap::gc:: -- --test-threads=3` | 24 passed, 0 failed | 0 |
| 31 | `cargo test --test executor -- --test-threads=3` | 13 passed, 0 failed | 0 |
| 32 | `cargo test --lib exec:: -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 33 | `cargo test --test executor_gc -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 34 | `cargo test --test gc_roots -- --test-threads=3` | 70 passed, 0 failed | 3 |
| 35 | `cargo test --lib heap::inventory:: -- --test-threads=3` | exit 101 during edit; 15 passed, 1 failed; rerun above: 16 passed | 0 |
| 36 | `cargo test --test need -- --test-threads=3` | 54 passed, 0 failed | 15 |
| 37 | `cargo test --lib task::need_tests -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 38 | `cargo test --test exceptions -- --test-threads=3` | 22 passed, 0 failed | 1 |
| 39 | `cargo run --example e2e` | passed | 0 |
| 40 | `cargo run --example e2e -- failure` | passed | 0 |
| 41 | `cargo run --example e2e -- cancel` | passed | 0 |
| 42 | `cargo run --example e2e -- companion` | passed | 0 |
| 43 | `cargo test --test e2e -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 44 | `cargo test --test native_fold -- --test-threads=3` | 16 passed, 0 failed | 1 |
| 45 | `cargo test --lib vm::native:: -- --test-threads=3` | 21 passed, 0 failed | 2 |
| 46 | `cargo test --test exception_waits -- --test-threads=3` | 54 passed, 0 failed | 0 |
| 47 | `cargo test --test concurrency -- --test-threads=3` | 17 passed, 0 failed | 0 |
| 48 | `cargo test --test concurrency seeded -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 49 | `cargo run --release --example bench` | passed | 66 |
| 50 | `cargo test --test measurement -- --test-threads=3` | 9 passed, 0 failed | 4 |
| 51 | `cargo test --test js_source intl_list_format -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 52 | `cargo test --test js_source intl_duration_format -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 53 | `cargo test --lib value::string::tests::string_to_number_budgeted -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 54 | `cargo test --test quickjs_intl -- --nocapture --test-threads=3` | 1 passed, 0 failed | 1 |
| 55 | `cargo test --test js_source using_declarations -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 56 | `cargo test --test js_source await_using -- --test-threads=3` | 4 passed, 0 failed | 4 |
| 57 | `cargo test --test js_source async_disposable_stack -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 58 | `cargo test --test js_source for_using_of_loops -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 59 | `cargo test --test exception_waits -- --test-threads=3` | 54 passed, 0 failed | 1 |
| 60 | `cargo test --test js_modules using_module_resources_dispose_after_evaluation -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 61 | `cargo test --test quickjs_language quickjs_operator_cases_run_unmodified -- --exact --nocapture --test-threads=3` | 1 passed, 0 failed | 0 |
| 62 | `cargo test --test exceptions quickjs_using -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 63 | `cargo test --test quickjs_language --test quickjs_functions --test quickjs_objects --test quickjs_builtins --test quickjs_collections --test quickjs_bigint --test quickjs_async --test quickjs_intl --test js_source --test js_functions --test js_modules --test exceptions --test error_stacks --test need --test native_fold --test cancellation --test malformed_input -- --test-threads=3` | 856 passed, 0 failed | 102 |
| 64 | `cargo test --test closures -- --test-threads=3` | 10 passed, 0 failed | 1 |
| 65 | `cargo run -- examples/demo.js` | passed | 0 |
| 66 | `cargo test --test native_unwind -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 67 | `cargo test --lib frontend::tests::arrow_functions_and_asi -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 68 | `cargo test --test js_functions arrow_function_values -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 69 | `cargo test --lib frontend::tests::comma_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 70 | `cargo test --test js_source comma_operator_evaluates_left_to_right -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 71 | `cargo test --lib frontend::tests::bitwise_and_shift_precedence -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 72 | `cargo test --test js_source bitwise_operators_follow_to_int32 -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 73 | `cargo test --lib frontend::tests::break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 74 | `cargo test --test js_source while_break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 75 | `cargo test --lib frontend::tests::exponent_operator_syntax -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 76 | `cargo test --test js_source exponent_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 77 | `cargo test --test exceptions break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 78 | `cargo test --test exceptions a_jump_through_finally -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 79 | `cargo test --test exception_waits jump -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 80 | `cargo test --test exception_waits a_break_through_finally -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 81 | `cargo test --lib frontend::tests::update_expressions -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 82 | `cargo test --test js_source update_expressions_on_bindings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 83 | `cargo test --test js_source update_expressions_on_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 84 | `cargo test --test need property_update_resumes_after_wait -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 85 | `cargo test --test gc_roots property_update_base_survives -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 86 | `cargo test --lib frontend::tests::do_while_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 87 | `cargo test --test js_source do_while_loops -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 88 | `cargo test --lib frontend::tests::for_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 89 | `cargo test --test js_source for_loops -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 90 | `cargo test --test js_source string_values -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 91 | `cargo test --test publication strings_publish -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 92 | `cargo test --test gc strings_in_frames -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 93 | `cargo test --lib value:: -- --test-threads=3` | 50 passed, 0 failed | 1 |
| 94 | `cargo test --test js_source strings_are_code_unit_sequences -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 95 | `cargo test --lib frontend::tests::unicode_string_escapes_decode_code_units -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 96 | `cargo test --test js_source addition_concatenates_strings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 97 | `cargo test --lib value::string:: -- --test-threads=3` | 21 passed, 0 failed | 1 |
| 98 | `cargo test --test js_source strings_ -- --test-threads=3` | 9 passed, 0 failed | 6 |
| 99 | `cargo test --test js_source strings_compare_by_code_units -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 100 | `cargo test --lib frontend::tests::labelled_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 101 | `cargo test --test js_source labelled_break_and_continue -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 102 | `cargo test --test exception_waits labelled_jumps_wait_and_resume_after_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 103 | `cargo test --lib frontend::tests::switch_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 104 | `cargo test --test js_source switch_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 105 | `cargo test --test exception_waits switch_waits_survive_migration_and_collection -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 106 | `cargo test --lib frontend::tests::compound_assignment -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 107 | `cargo test --test js_source compound_assignment_on_bindings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 108 | `cargo test --test js_source compound_assignment_on_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 109 | `cargo test --test js_source deep_property_compound_assignments -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 110 | `cargo test --test need property_compound_assignment_resumes_after_wait -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 111 | `cargo test --test gc_roots property_compound_assignment_base_survives -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 112 | `cargo test --test js_source typeof_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 113 | `cargo test --lib vm::dispatch::tests::typeof_classifies_every_object_kind -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 114 | `cargo test --lib frontend::tests::unary_binds_tighter_than_binary_and_looser_than_calls -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 115 | `cargo test --test js_source void_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 116 | `cargo test --lib frontend::tests::var_statements -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 117 | `cargo test --test js_source var_declarations_hoist_to_the_function -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 118 | `cargo test --test js_functions top_level_function_declarations_are_var_scoped -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 119 | `cargo test --lib frontend::tests::object_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 120 | `cargo test --test js_source object_literals_and_named_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 121 | `cargo test --test publication publishing_an_object_literal_is_rejected_with_property_path -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 122 | `cargo test --test gc_roots object_property -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 123 | `cargo test --lib frontend::tests::computed_member_access -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 124 | `cargo test --test js_source computed_member_access -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 125 | `cargo test --lib vm::property::tests::computed_keys_convert_by_to_property_key -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 126 | `cargo test --lib frontend::tests::directive_prologues_set_strictness -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 127 | `cargo test --test js_source use_strict_directives -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 128 | `cargo test --lib frontend::tests::default_parameters -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 129 | `cargo test --test js_functions default_parameters -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 130 | `cargo test --lib frontend::tests::legacy_octal_literals_in_sloppy_mode -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 131 | `cargo test --test js_source legacy_octal_literals_in_sloppy_scripts -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 132 | `cargo test --lib frontend::tests::legacy_octal_escapes_in_sloppy_mode -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 133 | `cargo test --test js_source legacy_octal_escapes_in_sloppy_scripts -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 134 | `cargo test --test js_source object_intrinsics -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 135 | `cargo test --test publication closures_using_builtin_globals_publish -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 136 | `cargo test --test executor two_workers_share_the_realm_intrinsics -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 137 | `cargo test --test gc_roots realm_intrinsics_retain -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 138 | `cargo test --lib realm:: -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 139 | `cargo test --lib frontend::tests::template_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 140 | `cargo test --test js_source template -- --test-threads=3` | 10 passed, 0 failed | 0 |
| 141 | `cargo test --test js_source tagged_template -- --test-threads=3` | 8 passed, 0 failed | 1 |
| 142 | `cargo test --lib frontend::tests::tagged_template_syntax -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 143 | `cargo test --test js_source method_calls_bind_this -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 144 | `cargo test --lib frontend::tests::this_expressions -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 145 | `cargo test --test need a_method_resumes_with_the_same_this_after_a_wait_and_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 146 | `cargo test --lib frontend::tests::array_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 147 | `cargo test --lib object::array -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 148 | `cargo test --test js_source array_literals -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 149 | `cargo test --test publication publishing_an_array_is_rejected_with_index_path -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 150 | `cargo test --test gc_roots array_element -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 151 | `cargo test --test js_source define_property -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 152 | `cargo test --lib vm::property::tests::inherited_read_only_properties_reject_writes -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 153 | `cargo test --lib vm::descriptor -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 154 | `cargo test --test js_source function_own_properties -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 155 | `cargo test --test publication function_own_properties_follow_publication_rule -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 156 | `cargo test --lib value::share::tests::a_function_gaining_properties_during_a_check_is_not_cached -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 157 | `cargo test --test gc_roots function_properties -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 158 | `cargo test --test js_source new_operator_constructs_objects -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 159 | `cargo test --test js_source a_constructor_suspends_at_a_need_and_completes_after_publication -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 160 | `cargo test --test need a_constructor_resumes_with_its_new_object_after_a_wait_and_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 161 | `cargo test --lib frontend::tests::new_expressions -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 162 | `cargo test --test js_source prototype_reflection -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 163 | `cargo test --lib vm::prototype -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 164 | `cargo test --test js_source object_extensibility -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 165 | `cargo test --test publication freezing_keeps_ownership_and_published_functions_are_frozen -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 166 | `cargo test --lib object:: -- --test-threads=3` | 27 passed, 0 failed | 3 |
| 167 | `cargo test --test js_source own_property_key_order -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 168 | `cargo test --lib vm::keys -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 169 | `cargo test --test js_source instanceof_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 170 | `cargo test --lib frontend::tests::binary_precedence_and_left_associativity -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 171 | `cargo test --test js_source delete_property_references -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 172 | `cargo test --lib frontend::tests::delete_of_identifiers_is_rejected -- --test-threads=3` | 0 matched; retired filter, replaced above | 0 |
| 173 | `cargo test --test js_source in_operator -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 174 | `cargo test --lib frontend::tests::for_in_heads -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 175 | `cargo test --test js_source for_in_over_own_properties -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 176 | `cargo test --test gc_roots for_in -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 177 | `cargo test --test js_source for_in_walks_the_prototype_chain -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 178 | `cargo test --lib vm::keys:: -- --test-threads=3` | 6 passed, 0 failed | 0 |
| 179 | `cargo test --test js_source for_in_over_arrays -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 180 | `cargo test --lib frontend::tests::for_var_heads -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 181 | `cargo test --test js_source var_heads_in_for_loops -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 182 | `cargo test --lib frontend::tests::sloppy_for_in_var_initializer -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 183 | `cargo test --test js_source sloppy_for_in -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 184 | `cargo test --test js_source methods_on_primitive_bases -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 185 | `cargo test --test js_source methods_on_primitive_bases_allocate_no_wrappers -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 186 | `cargo test --test js_functions call_apply_and_reflect_apply -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 187 | `cargo test --test js_functions forwarded_calls_wait_and_resume_on_another_thread -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 188 | `cargo test --test js_source native_error_bound_constructor_prototypes -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 189 | `cargo test --test js_functions function_length_and_name -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 190 | `cargo test --test js_functions function_metadata_property_operations -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 191 | `cargo test --test js_source array_push -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 192 | `cargo test --test js_source symbol_to_primitive -- --test-threads=3` | 4 passed, 0 failed | 3 |
| 193 | `cargo test --test exception_waits symbol_to_primitive -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 194 | `cargo test --test js_source objects_convert_with_to_primitive -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 195 | `cargo test --test js_source object_conversions_cover_hints_operators_and_property_keys -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 196 | `cargo test --test need to_primitive -- --test-threads=3` | 3 passed, 0 failed | 1 |
| 197 | `cargo test --test exception_waits to_primitive -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 198 | `cargo test --test js_source primitive_wrapper -- --test-threads=3` | 8 passed, 0 failed | 3 |
| 199 | `cargo test --test js_functions arrows_capture_this -- --test-threads=3` | 2 passed, 0 failed | 2 |
| 200 | `cargo test --test need an_arrow_retains_lexical_this_after_a_wait_collection_and_migration -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 201 | `cargo test --test js_source string_length_and_index_properties -- --test-threads=3` | 1 passed, 0 failed | 2 |
| 202 | `cargo test --test js_source string_index_reads_allocate_no_wrappers -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 203 | `cargo test --test js_functions arguments_ -- --test-threads=3` | 9 passed, 0 failed | 0 |
| 204 | `cargo test --lib frontend::tests::object_literal_methods -- --exact --test-threads=3` | 1 passed, 0 failed | 3 |
| 205 | `cargo test --test js_source object_literal_methods -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 206 | `cargo test --test publication method_home_object_prevents_publication -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 207 | `cargo test --test gc_roots method_home_object -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 208 | `cargo test --test js_source parse_ -- --test-threads=3` | 15 passed, 0 failed | 8 |
| 209 | `cargo test --lib frontend::tests::accessor_literals -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 210 | `cargo test --test js_source accessor -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 211 | `cargo test --test need getter_waits_on_dependency_and_resumes -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 212 | `cargo test --test need accessor_cancellation_and_dependency_failure_unwind -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 213 | `cargo test --test gc_roots accessor -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 214 | `cargo test --test publication accessor -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 215 | `cargo test --test js_source math_ -- --test-threads=3` | 8 passed, 0 failed | 2 |
| 216 | `cargo test --test js_source math_pow -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 217 | `cargo test --test js_source global_object_bindings -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 218 | `cargo test --test js_functions global_ -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 219 | `cargo test --test gc_roots global_object -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 220 | `cargo test --test js_source stored_proto_accessor -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 221 | `cargo test --test publication built_in_proto_accessors_are_shared_across_tasks -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 222 | `cargo test --lib vm::prototype:: -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 223 | `cargo test --test js_source sloppy_implicit_globals_and_delete -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 224 | `cargo test --test js_source error_ -- --test-threads=3` | 17 passed, 0 failed | 2 |
| 225 | `cargo test --test exceptions uncaught_error_objects_report_name_and_message -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 226 | `cargo test --test js_source string_split -- --test-threads=3` | 4 passed, 0 failed | 6 |
| 227 | `cargo test --test js_source string_code_unit_coercion_waits_yields_collects_and_migrates -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 228 | `cargo test --test error_stacks -- --test-threads=3` | 9 passed, 0 failed | 1 |
| 229 | `cargo test --test js_source array_constructor_and_is_array -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 230 | `cargo test --test js_source native_error -- --test-threads=3` | 3 passed, 0 failed | 2 |
| 231 | `cargo test --test js_source json_parse -- --test-threads=3` | 9 passed, 0 failed | 11 |
| 232 | `cargo test --test js_source symbol_ -- --test-threads=3` | 20 passed, 0 failed | 17 |
| 233 | `cargo test --test publication symbols_publish_with_identity -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 234 | `cargo test --lib vm::keys::tests::symbol_keys_follow_all_strings_in_creation_order -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 235 | `cargo test --test js_source json_stringify -- --test-threads=3` | 17 passed, 0 failed | 14 |
| 236 | `cargo test --test js_modules json_stringify_namespace_snapshot_limits -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 237 | `cargo test --test js_source symbol_registry -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 238 | `cargo test --test executor symbol_registry_is_shared_across_tasks -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 239 | `cargo test --test gc_roots symbol_registry_retains_unreferenced_symbols_across_collection -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 240 | `cargo test --lib regexp::parse::tests:: -- --test-threads=3` | 11 passed, 0 failed | 0 |
| 241 | `cargo test --lib bigint -- --test-threads=3` | 18 passed, 0 failed | 0 |
| 242 | `cargo test --test js_source bigint -- --test-threads=3` | 32 passed, 0 failed | 9 |
| 243 | `cargo test numeric_separator -- --test-threads=3` | 3 passed, 0 failed | 10 |
| 244 | `cargo test --test js_source bigint_ -- --test-threads=3` | 32 passed, 0 failed | 9 |
| 245 | `cargo test --lib value::bigint:: -- --test-threads=3` | 16 passed, 0 failed | 0 |
| 246 | `cargo test --test atoms bigint_compare_and_swap -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 247 | `cargo test --test js_modules bigint_namespace_definitions_use_same_value -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 248 | `cargo test --lib regexp::matcher::tests:: -- --test-threads=3` | 16 passed, 0 failed | 0 |
| 249 | `cargo test --lib vm::date::tests::date_time_arithmetic -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 250 | `cargo test --test js_source date_ -- --test-threads=3` | 32 passed, 0 failed | 8 |
| 251 | `cargo test --lib vm::date::tests::date_utc -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 252 | `cargo test --test js_source date_to_iso_string -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 253 | `cargo test --test js_source date_utc_fields -- --test-threads=3` | 3 passed, 0 failed | 1 |
| 254 | `cargo test --test js_source float_typed_array -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 255 | `cargo test --lib object::typed_array::tests:: -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 256 | `cargo test --test js_source regexp_ -- --test-threads=3` | 28 passed, 0 failed | 1 |
| 257 | `cargo test --test js_source overlapping_capture_output -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 258 | `cargo test --test js_source typed_array_ -- --test-threads=3` | 24 passed, 0 failed | 3 |
| 259 | `cargo test --test js_source for_of_ -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 260 | `cargo test --test exception_waits for_of_ -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 261 | `cargo test --test js_source string_replace -- --test-threads=3` | 6 passed, 0 failed | 3 |
| 262 | `cargo test --test js_source regexp_unicode_mode -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 263 | `cargo test --test promises -- --test-threads=3` | 60 passed, 0 failed | 2 |
| 264 | `cargo test --test js_source promise_thenable_adoption -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 265 | `cargo test --test date_host -- --test-threads=3` | 11 passed, 0 failed | 0 |
| 266 | `cargo test --test js_source intl_number_format -- --test-threads=3` | 17 passed, 0 failed | 2 |
| 267 | `cargo test --test js_source date_ -- --test-threads=3` | 32 passed, 0 failed | 7 |
| 268 | `cargo test --test js_source regexp_case_insensitive_unicode -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 269 | `cargo test --test js_source regexp_case_insensitive_legacy -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 270 | `cargo test --test js_source intl_plural_rules -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 271 | `cargo test --test sim_host intl_default_locale_resolution -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 272 | `cargo test --test js_source regexp_property_escapes -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 273 | `cargo test --lib vm::date:: -- --test-threads=3` | 10 passed, 0 failed | 3 |
| 274 | `cargo test --test js_source spread_ -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 275 | `cargo test --test need iterator_next_waits_on_dependency -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 276 | `cargo test --lib frontend::tests::spread_forms -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 277 | `cargo test --test js_source rest_ -- --test-threads=3` | 7 passed, 0 failed | 1 |
| 278 | `cargo test --test js_source array_and_object_destructuring_waits_collects_and_migrates -- --test-threads=3` | 1 passed, 0 failed | 3 |
| 279 | `cargo test --test js_source array_from_ -- --test-threads=3` | 4 passed, 0 failed | 1 |
| 280 | `cargo test --test need array_from_iterator_waits_on_dependency -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 281 | `cargo test --lib regexp:: -- --test-threads=3` | 32 passed, 0 failed | 1 |
| 282 | `cargo test --test js_source regexp_unicode_string_properties -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 283 | `cargo test --test js_source array_destructuring -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 284 | `cargo test --test js_source generator_ -- --test-threads=3` | 12 passed, 0 failed | 15 |
| 285 | `cargo test --test exception_waits generator_ -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 286 | `cargo test --test js_source weak_map -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 287 | `cargo test --test gc weak_map_ -- --test-threads=3` | 9 passed, 0 failed | 0 |
| 288 | `cargo test --test publication weak_maps_ -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 289 | `cargo test --test js_source intl_collator -- --test-threads=3` | 13 passed, 0 failed | 2 |
| 290 | `cargo test --test js_source destructuring -- --test-threads=3` | 7 passed, 0 failed | 4 |
| 291 | `cargo test --lib frontend::tests::computed_property_names -- --exact --test-threads=3` | 1 passed, 0 failed | 3 |
| 292 | `cargo test --test js_source computed_prop -- --test-threads=3` | 5 passed, 0 failed | 3 |
| 293 | `cargo test --test need computed_property_names_wait_and_migrate -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 294 | `cargo test --test js_source indirect_eval -- --test-threads=3` | 6 passed, 0 failed | 2 |
| 295 | `cargo test --test js_source indirect_eval_on_small_worker_stacks -- --exact --test-threads=3` | 1 passed, 0 failed | 1 |
| 296 | `cargo test --test js_source math_sum_precise -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 297 | `cargo test --test need math_sum_precise_waits_and_migrates -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 298 | `cargo test --lib task::sim_tests:: -- --test-threads=3` | 12 passed, 0 failed | 0 |
| 299 | `cargo test --test sim_facilities -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 300 | `cargo test --test js_source weak_ref -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 301 | `cargo test --test gc weak_ref -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 302 | `cargo test --test gc kept_objects -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 303 | `cargo test --test publication weak_refs -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 304 | `cargo test --test js_source primitive_wrappers_survive_wait_collection_and_migration -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 305 | `cargo test --test gc weak_map_symbol_keys_follow_traced_reachability -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 306 | `cargo test --test quickjs_async -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 307 | `cargo test --test js_source direct_eval -- --test-threads=3` | 10 passed, 0 failed | 3 |
| 308 | `cargo test --test js_source base_classes -- --test-threads=3` | 7 passed, 0 failed | 0 |
| 309 | `cargo test --lib frontend::tests::class_syntax -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 310 | `cargo test --test js_source derived_classes -- --test-threads=3` | 8 passed, 0 failed | 0 |
| 311 | `cargo test --test need derived_constructor_waits_inside_super -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 312 | `cargo test --test js_source quickjs_date_regexp_cases_run_unmodified -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 313 | `cargo test --test quickjs_collections -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 314 | `cargo test --test quickjs_bigint -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 315 | `cargo test --test js_functions -- --test-threads=3` | 61 passed, 0 failed | 2 |
| 316 | `cargo test --lib frontend:: -- --test-threads=3` | 103 passed, 0 failed | 12 |
| 317 | `cargo test --test js_modules -- --test-threads=3` | 89 passed, 0 failed | 4 |
| 318 | `cargo test --test js_source super_property_access -- --test-threads=3` | 6 passed, 0 failed | 0 |
| 319 | `cargo test --test need super_property_access_waits_collects_and_migrates -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 320 | `cargo test --test js_source object_constructor -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 321 | `cargo test --lib module::namespace:: -- --test-threads=3` | 4 passed, 0 failed | 0 |
| 322 | `cargo test --test js_functions super_ -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 323 | `cargo test --test js_functions function_constructor -- --test-threads=3` | 3 passed, 0 failed | 0 |
| 324 | `cargo test --test quickjs_objects quickjs_object_cases_run_unmodified -- --exact --nocapture --test-threads=3` | 1 passed, 0 failed | 1 |
| 325 | `cargo test --test js_source proxy_ -- --test-threads=3` | 12 passed, 0 failed | 2 |
| 326 | `cargo test --lib heap::inventory::proxy_edges_retain_and_reclaim -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 327 | `cargo test --test js_source logical_assignment -- --test-threads=3` | 2 passed, 0 failed | 0 |
| 328 | `cargo test --test need logical_assignment -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 329 | `cargo test --lib frontend::tests::nullish_and_logical_assignment_grammar -- --test-threads=3` | 1 passed, 0 failed | 0 |
| 330 | `cargo test --test js_source array_mutators -- --test-threads=3` | 9 passed, 0 failed | 1 |
| 331 | `cargo test --test need array_mutators -- --test-threads=3` | 2 passed, 0 failed | 2 |
| 332 | `cargo test --test js_source array_search -- --test-threads=3` | 3 passed, 0 failed | 2 |
| 333 | `cargo test --test need array_search -- --test-threads=3` | 2 passed, 0 failed | 1 |
| 334 | `cargo test --test js_source array_callbacks -- --test-threads=3` | 3 passed, 0 failed | 7 |
| 335 | `cargo test --test need array_callbacks -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 336 | `cargo test --test async_generators -- --test-threads=3` | 9 passed, 0 failed | 2 |
| 337 | `cargo test --test class_static_blocks -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 338 | `cargo test --test js_source private_class -- --test-threads=3` | 5 passed, 0 failed | 0 |
| 339 | `cargo test --test async_iteration --test async_generators -- --test-threads=3` | 16 passed, 0 failed | 5 |
| 340 | `cargo test --test js_source array_copy -- --test-threads=3` | 6 passed, 0 failed | 6 |
| 341 | `cargo test --test need array_copy -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 342 | `cargo test --test map_iterables -- --test-threads=3` | 5 passed, 0 failed | 1 |
| 343 | `cargo test --test need map_iterable_ -- --test-threads=3` | 2 passed, 0 failed | 3 |
| 344 | `cargo test --lib vm::native::add_entries::tests -- --test-threads=3` | 6 passed, 0 failed | 5 |
| 345 | `cargo test --test sets --test map_iterables -- --test-threads=3` | 13 passed, 0 failed | 2 |
| 346 | `cargo test --test boolean_bigint_wrappers -- --test-threads=3` | 8 passed, 0 failed | 1 |
| 347 | `cargo test --test js_source regexp_script_property_escapes -- --exact --test-threads=3` | 1 passed, 0 failed | 0 |
| 348 | `cargo test --test builtin_subclasses -- --test-threads=3` | 17 passed, 0 failed | 1 |
| 349 | `cargo test --test js_source string_ -- --test-threads=3` | 52 passed, 0 failed | 15 |
| 350 | `cargo test --lib string_methods:: -- --test-threads=3` | 1 passed, 0 failed | 1 |
| 351 | `cargo test --test object_statics -- --test-threads=3` | 6 passed, 0 failed | 2 |
| 352 | `cargo test --test need object_statics_wait_collect_and_migrate -- --test-threads=3` | 1 passed, 0 failed | 2 |
| 353 | `cargo test --test js_source regexp_ -- --test-threads=3` | 28 passed, 0 failed | 2 |
| 354 | `cargo test --test js_source string_replace -- --test-threads=3` | 6 passed, 0 failed | 3 |
| 355 | `cargo test --test math -- --test-threads=3` | 8 passed, 0 failed | 2 |
| 356 | `cargo test --test js_source math_ -- --test-threads=3` | 8 passed, 0 failed | 2 |
| 357 | `cargo test --test reflect -- --test-threads=3` | 16 passed, 0 failed | 3 |
| 358 | `cargo test --test object_to_string -- --test-threads=3` | 6 passed, 0 failed | 1 |
| 359 | `cargo build --release` | passed | 25 |
| 360 | `cargo run -- examples/demo.js` | passed | 0 |
| 361 | `cargo run --release --example bench -- --quick` | passed | 3 |
| 362 | `cargo run --release --example bench -- memory` | passed | 0 |
| 363 | `cargo run --release --example bench -- work --workers 4 --rounds 9` | passed | 2 |
| 364 | `cargo run --release --example bench -- strings --rounds 3` | passed | 2 |
| 365 | `cargo test --test measurement string_concatenation_is_amortized_linear -- --test-threads=3` | 1 passed, 0 failed | 5 |

</details>

### Post-rebase finish checks

Run at `0e5bc989`, rebased onto master `72201c8b`. Every command used the default
target directory, with no peer workers. The later result-recording changes are
documentation only.

| Exact command | Observed result |
| --- | --- |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --all-targets -- --test-threads=3` | **1940 passed, 0 failed, 0 ignored**, 57 result groups (including empty binary/example groups); 145 s wall time |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --doc -- --test-threads=3` | **6 passed**, including compile-fail root/scope lifetime checks |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --lib heap::inventory:: -- --test-threads=3` | **16 passed** |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo test --lib frontend::tests::grammar_doc_examples -- --exact --test-threads=3` | **1 passed**, 351 preserved example fences |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo fmt --all -- --check` | passed |
| `CARGO_BUILD_JOBS=3 nix develop -c cargo clippy --all-targets -- -D warnings` | passed |

No code defect follow-up was needed. This is a verification record, not a second
backlog; future implementation work remains in Pellets.

### Review corrections

The required Sol review returned fix_first with five documentation findings.
The full-suite record was committed while review ran; the remaining corrections
restore sloppy receiver/unmapped-arguments limitations, restrict RegExp execution
to lookahead, document cancellation's required enqueue and correct Task::run's
unlimited-budget/no-host signature. No runtime code or assertion changed.
The root inventory (16) and grammar examples (1) passed again, as did these
focused checks, each with CARGO_BUILD_JOBS=3 inside nix develop:

| Exact command | Result |
| --- | --- |
| `cargo test --lib regexp::matcher::tests::unsupported_features_fail_explicitly -- --exact --test-threads=3` | 1 passed |
| `cargo test --test js_functions call_apply_and_reflect_apply -- --exact --test-threads=3` | 1 passed |
| `cargo test --test js_functions arguments_object_holds_every_argument -- --exact --test-threads=3` | 1 passed |
| `cargo test --lib task::cancel_tests::cancelled_parked_consumer_leaves_the_cell_when_its_run_cleans_up -- --exact --test-threads=3` | 1 passed |

Formatter check and warnings-denied Clippy passed again after these corrections.
All five findings were fixed; no runtime follow-up was filed.
