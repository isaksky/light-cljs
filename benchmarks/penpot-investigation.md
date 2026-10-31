# Penpot compiler performance investigation — October 16–17, 2026

See the [October 27 regression check and repair](#default-v8-regression-check-october-27-2026)
for the later default-compiler slowdown and restored Penpot timings, and the
[October 29 comparison with shadow-cljs, cached and uncached](#re-measured-against-shadow-cljs-cached-and-uncached-october-29-2026-at-0ad2514f)
for the latest numbers. The
[October 30 Pants-build profile of `app.main`](#where-the-pants-builds-time-goes-on-appmain-october-30-2026-at-9907e916)
is the baseline the real-core redesign (docs/real-core-design.md) is measured
against.

Latest validated production rerun (phase112): **21.97 s / 0.948 GB RSS** for
main none, **28.24 s / 1.178 GB** for advanced, and **3.73 s / 0.498 GB** for
render. Fresh Shadow dev is **32.08 s / 2.551 GB** and release is **59.12 s /
3.901 GB**. Native clean compilations now use the pinned, quota-aware V8 backend
by default; interactive sessions and WebAssembly retain QuickJS. Main takes
31.5% less wall time and 62.8% less RSS than Shadow dev. All experimental engine
settings were unset. These final runs use the main working tree's rebuilt binary;
its complete outputs match the validated candidate byte-for-byte.

Default-backend validation passes **1,442 tests**, QuickJS-only passes **1,406**,
with zero failures and 106 ignored in each. Both pass strict Clippy; formatting,
three Node workflows, the source-engine build, ABI rejection check and WebAssembly
playground checks also pass. All Penpot outputs are byte-identical to phase111
and pass ESM syntax checks; the port is unchanged. Penpot has not been bundled
and run in a browser; [docs/penpot-browser-bundle.md](../docs/penpot-browser-bundle.md) records what that would take. [Phase112 evidence](results/compiler-2026-10-16/penpot-phase112/README.md).

The native backend and release defaults are promoted. A final collection-store
specialization experiment yielded only a 0.7% average timing reduction and was
not promoted; [phase113](results/compiler-2026-10-16/penpot-phase113/README.md)
records the controlled comparison. Further large gains are not established by
the current profiling evidence.

## What each collection traces, and pacing by its work (light-cljs-664, October 30, 2026, on `0c78606c`)

**Summary.** A temporary per-collection log (not committed) on Penpot
`app.main` at 1 thread found two reasons each Pants-build collection cost
more as the build went on. First, every dependency cell the runtime had
ever made was a root source, so each collection upgraded, visited and
released all of them: 240,000 by the end, 3-5 ms of an 18-24 ms
collection, while only about 400 hold a heap value. Now a cell becomes a
root source only when its outcome holds an object or a symbol
(`crates/big-boy-pants-js/src/dep.rs`). That change is landed: garbage
collection falls from 18.3% to 15.0% of the main thread's samples, with
the same collections (631) and the same peak memory. Second, the
allocation-count trigger waits as many allocations as there are live
objects (65,000) while a collection traces 2.2 times as many edges. Pacing
by traced edges halved the collections and brought collection to 10.7%,
but it raised peak memory and broke a memory-budget test, so it is not
landed (below; light-cljs-674). The 8% target is missed.

Setup as in the October 30 profile below: release builds (thin LTO, one
codegen unit, `CARGO_BUILD_JOBS=4`) of `0c78606c` ("before"), of
`0c78606c` with both changes ("both", the experiment) and of this commit
("landed", the root-source change only); the same command and budgets.
Before and both alternated over three rounds; landed ran three rounds
afterwards. Medians [range]; peak memory is `/usr/bin/time -l`'s maximum
resident set size, GB = 1e9 bytes. Other workers built and tested
throughout, and this pellet's own suites ran during the landed rounds
(1-minute loads up to 32), so its wall times are not comparable;
collections and peak memory do not depend on load.

| Penpot `app.main` | Wall | Peak memory | Collections | 1-min loads |
| --- | ---: | ---: | ---: | --- |
| 1 thread, before | 88.4 s [76.6-90.7] | 1.15 GB | 631 | 5.4, 14.7, 16.8 |
| 1 thread, landed | (89.9 s [73.9-91.5]) | 1.14 GB | 631 | 7.8, 21.7, 32.2 |
| 1 thread, both | 71.1 s [70.9-72.1] | 1.22 GB | 295 | 20.8, 15.4, 20.1 |
| 4 threads, before | 79.8 s [76.8-83.9] | 1.37 GB | 350-368 | 20.3, 16.3, 19.5 |
| 4 threads, landed | (94.3 s [75.2-102.4]) | 1.36 GB | 358-377 | 14.3, 28.5, 24.9 |
| 4 threads, both | 69.4 s [66.3-78.9] | 1.68 GB | 124-127 | 17.3, 14.8, 23.4 |

Every build still stops at `core.MapEntry` (light-cljs-643). Garbage
collection's share of the main thread (`/usr/bin/sample`, 1 ms, one
sampled run each at 1 thread; `bb benchmarks/sample-inclusive.bb
--buckets "gc=Heap>::mark_and_sweep;interp=execute_budgeted"`): before
18.3% (load 10.3), landed 15.0% (load 28.3), both 10.7% (load 10.8).

Generated projects (`generate.bb --seed 42`, `release bench --no-cache
--stats`, the projects' 2 GiB budget); every build of a size wrote the
same output (`1a75bc87b26f3121…` at 200k, `6741d7c256b43ab4…` at 800k):

| | Wall | Peak memory | Collections | 1-min loads |
| --- | ---: | ---: | ---: | --- |
| 200k, 1 thread, before | 18.7 s [16.2-23.3] | 1.25 GB | 58 | 24.5, 23.0, 23.0 |
| 200k, 1 thread, landed | (20.7 s [18.2-25.2]) | 1.26 GB | 58 | 13.0, 28.0, 26.2 |
| 200k, 1 thread, both | 18.9 s [16.9-22.9] | 1.35 GB | 28 | 22.8, 22.6, 21.3 |
| 200k, 4 threads, before | 13.4 s [12.0-15.5] | 1.56 GB | 52-62 | 21.9, 23.8, 20.2 |
| 200k, 4 threads, landed | (13.9 s [12.0-16.1]) | 1.56 GB | 47-59 | 21.2, 29.5, 31.7 |
| 200k, 4 threads, both | 12.4 s [12.0-13.9] | 1.69 GB | 23-28 | 23.7, 23.1, 20.8 |
| 800k, 1 thread, before | 70.3 s [65.5-79.7] | 4.78 GB | 84 | 20.0, 10.2, 2.8 |
| 800k, 1 thread, both | 73.2 s [68.3-75.6] | 4.76 GB | 43 | 15.2, 7.1, 2.3 |
| 800k, 4 threads, before | fails | 2.19-2.42 GB | 251-468 | 12.0, 4.4, 8.6 |
| 800k, 4 threads, both | fails | 2.18-2.20 GB | 207-572 | 12.5, 3.6, 10.3 |

The landed change was not run at 800k. At 800k and 4 threads both before
and both exceed the project's 2 GiB budget (`heap byte or string limit
exceeded`) after about 1,175 namespaces and 196,000 macro calls. The
generated projects' collections are cheap (small live heaps), so halving
them does not move wall time beyond this machine's noise.

**Why pacing by traced edges is not landed.** It made each collection wait
as many allocations as the last one traced edges (`HeapPolicy::threshold`
taking the larger of the live objects and the edges). Peak memory rose 6%
at 1 thread and 22% at 4 threads on Penpot, against the 15% allowed, and
`bbp_evaluator::tests::namespace_macro_chains_fit_the_default_heap_with_linear_live_bytes`
failed: the object table's slot segments are charged to the byte budget
and never shrink, so more garbage between collections leaves 1.5 MB more
"live" bytes at 400 calls (23.2 MB against the test's 22.0 MB bound; the
unchanged build leaves 21.6 MB). What remains at 1 thread (from the log,
with both changes): 5.0 s of pauses, 4.2 s of them marking at about 230 ns
per live object, and 2.5 s freeing collected objects after each pause on
the collecting thread, about 4% of the build by itself. light-cljs-674
takes it from here: release or stop charging empty slot segments before
pacing by edges, free collected objects off the collecting thread, and
fold the per-object conditional pass into the mark.

## Effect-lane waits on app.main (October 30, 2026, at `0c78606c` and `95b5909e`)

**Summary.** light-cljs-665 asked which compile-time state the Pants build's
lane waits protect on Penpot `app.main`, and whether the earlier units they
wait for can touch it. None of the waits were atom accesses: 466 of 496
parked accesses at 4 threads (67.7 s of 68.6 s summed, and the 20 s longest
wait behind the grammar macro) were a macro's `binding` installing its
overlay; the other 30 were shared initializers selecting their caller (1 s).
An experiment build without the binding lane (the "no binding lane" rows
below) halved the waits at 4 and 8 threads, but the waits moved to atom
accesses: 209 of 243 are a multimethod's method table read during dispatch
(the port keeps MultiFn tables in atoms; the caller is rumext's
`compile-form` from the UI namespaces' macros), the longest still the whole
grammar macro, so wall time did not change (the build also still stops at
the end of that macro, light-cljs-643). In hindsight the grammar macro
touched no atom, table or hierarchy (its 20,559 lane checks were shared
mutable fields and initializer selections), but a running macro can reach any
loaded Var by name, so nothing proves it cannot; letting a lane skip it needs
light-cljs-446's require-closure rule (light-cljs-672).

The binding lane stayed until light-cljs-676 (below, "Effect-lane waits on
app.main after the realization gate"). A binding's overlay is task-local, but a published
lazy sequence or delay is realized by the first task that needs it, under that
task's bindings, so a later task's binding could decide its value; review found
this, and `published_once_cells_keep_source_order_bindings`
(tests/bbp_macros.rs) now guards it. The lane does not cover a later caller
that forces such a cell with no binding of its own; an engine realization gate
that lets the earliest caller in source order realize every shared once-cell
fixes both and would let the binding lane go. It was prototyped, but it changes
what the light-cljs-601/613 scheduling tests assert, so it is
light-cljs-676.

Setup. Release builds of `0c78606c` (with the binding lane) and the experiment
(`0c78606c` without it, branch `light-cljs-665-gate-prototype`, 1247a9f7),
`--no-default-features --features bbp-macros`, thin LTO, `CARGO_BUILD_JOBS=4`;
the command of the October 30 profile below (`--jobs N`, every budget,
`--stats`, source map). Apple M3 Ultra (Virtual), 24 logical CPUs, macOS
15.7.3, Rust 1.99.1. Peers built and ran test suites throughout: the
1-minute load average before each run was 5.4-25.4, so ranges are wide.
The two alternated per thread count, three rounds; medians [range]. Every
run stops at the grammar macro's `core.MapEntry`, as before.

| Build | Threads | Wall s | User CPU s | Lane waits | Summed wait s | Longest wait s | 1-min loads |
| --- | --- | ---: | ---: | ---: | ---: | ---: | --- |
| binding lane | 1 | 75.0 [69.7-87.2] | 72.8 [68.1-77.0] | 0 | 0 | 0 | 5.7, 6.0, 17.8 |
| no binding lane | 1 | 79.6 [71.4-90.9] | 74.3 [69.8-76.3] | 0 | 0 | 0 | 9.6, 6.4, 20.7 |
| binding lane | 2 | 76.0 [66.9-85.4] | 77.8 [69.6-80.3] | 175 [171-181] | 32.3 [28.6-41.9] | 17.1 [15.7-26.4] | 8.1, 20.6, 19.0 |
| no binding lane | 2 | 75.3 [66.2-87.4] | 77.2 [68.9-84.2] | 172 [165-173] | 32.6 [28.1-37.3] | 17.5 [15.2-18.3] | 9.2, 18.9, 23.3 |
| binding lane | 4 | 76.1 [64.3-83.4] | 95.1 [78.5-95.3] | 489 [487-507] | 69.2 [58.2-75.1] | 18.2 [15.1-19.7] | 8.3, 16.7, 25.4 |
| no binding lane | 4 | 74.8 [64.9-82.4] | 99.0 [85.1-103.0] | 256 [238-267] | 52.6 [44.7-59.5] | 17.5 [15.3-17.9] | 6.3, 15.3, 22.0 |
| binding lane | 8 | 78.4 [67.9-83.5] | 119.3 [103.2-120.4] | 639 [619-653] | 123.8 [105.6-136.0] | 16.8 [15.4-17.8] | 6.0, 15.5, 20.7 |
| no binding lane | 8 | 76.0 [75.8-85.9] | 141.4 [132.7-149.3] | 310 [308-335] | 83.7 [80.5-92.6] | 18.2 [16.5-21.8] | 5.4, 15.7, 16.7 |

The coordinator's blocked share, at `95b5909e` (master when this was
measured; its lanes and binding lane are `0c78606c`'s), three sampled runs
per thread count (`/usr/bin/sample`, 1 ms, started 2 s into the run;
`bb benchmarks/sample-inclusive.bb` buckets on the main thread, a sample
counted blocked when its innermost frame is a condition-variable, lock,
semaphore or Mach-message wait). Sampling slows the build, so wall times here
are not comparable with the table above; peers built and ran suites
throughout.

| Threads | Blocked, runs 1/2/3 | In `claim_help` (drain waits) | Lane waits | Longest wait s | Wall s (sampled) | 1-min loads |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 3.2, 2.6, 2.7% | 0, 0, 0% | 0, 0, 0 | 0 | 96.8, 92.5, 95.4 | 9.0, 17.3, 15.3 |
| 2 | 57.3, 55.7, 55.0% | 10.7, 9.7, 9.9% | 172, 170, 175 | 31.2, 21.1, 18.7 | 126.7, 90.9, 84.9 | 28.1, 15.7, 14.7 |
| 4 | 70.5, 68.3, 67.8% | 19.5, 19.9, 18.0% | 505, 490, 492 | 39.7, 23.1, 22.0 | 138.1, 93.4, 88.5 | 27.4, 15.6, 10.2 |
| 8 | 67.7, 69.0, 69.0% | 20.9, 20.5, 20.5% | 636, 617, 615 | 18.3, 25.7, 25.0 | 87.4, 102.9, 107.9 | 18.1, 14.3, 12.3 |

At 1 thread the coordinator does all the work and is blocked about 3% of
the time (the October 30 profile below puts the joins of JavaScript compiled
on a helper thread at 3.1%). From 2 threads on it is blocked 55-70% of the time: about
10-20 points waiting for drained units other threads analyze, the rest
mostly in lane waits for the grammar macro. The experiment without the
binding lane (one sampled run each, at `0c78606c`) was blocked 56.5% at 2
threads, 67.0% at 4 and 69.7% at 8: the wait only moved to the multimethod
table.

What the waits touched (one traced 4-thread run each, a temporary build that
tagged every lane check by kind and logged the earliest unfinished units;
not in the tree): with the binding lane, 466 binding installations and 30
initializer selections, 0 atom accesses; without it, 215 atom accesses (209 of them
multimethod method tables in dispatch, the rest other untagged atoms) and 28
initializer selections. 494 of the 496 waits with the lane and 242 of the
243 without it waited for a unit that runs macros; skipping macro-free units (which can
never enter an engine) would have saved 0.02 s, so it is not done.

Output. `app.render` (JavaScript and source map) from the experiment build,
and from the gate prototype of light-cljs-676, at 1, 2, 4 and 8 threads is
byte-identical to the V8 build's (`72868c0c…`,
`e249c54d…` with this run's paths); so is the seed-42 200k project
(`workload.main`, `--optimize none`: `30f2cb4d…`, `6e9112c2…`).

## Where the Pants build's time goes on app.main (October 30, 2026, at `9907e916`)

**Summary.** light-cljs-635 profiled the Pants build (docs/glossary.md) on
Penpot `app.main` up to its current stop (`core.MapEntry` in instaparse's
grammar macro, light-cljs-643). One macro, `instaparse.abnf/abnf-parser`,
took 37-41 s of the 86.9 s one-thread build, and 25-28 s of that was garbage
collection paced by a cost that grew quadratically: every string append
charged a credit for a flattened copy of the whole string, and the credits
counted as allocation volume. The fix (below) takes the one-thread build
from 86.9 s to 66.5 s (-24%) and the four-thread build from 95.5 s to
60.6 s (-37%); four threads is now faster than one, but only 1.1x. What
remains, ranked by what survives the switch to the real `cljs.core`
(docs/real-core-design.md): interpreter speed (65% of the main thread),
garbage collection (19%) and, at 4 threads, waits on effect lanes. The
Pants build is 8.3x slower than V8 on the namespaces both builds reach,
7.0x without the grammar macro.

Setup. light-cljs master `9907e916` plus this change, release builds
(`cargo build --locked --release`, thin LTO, one codegen unit, as the
earlier Penpot numbers; `CARGO_BUILD_JOBS=4`): the Pants build
(`--no-default-features --features bbp-macros`), copied to fixed paths
before and after the change, and the V8 build (default features, V8 from
the machine cache). Apple M3 Ultra (Virtual), 24 logical CPUs, 32 GiB,
macOS 15.7.3, Rust/Cargo 1.99.1. The command is light-cljs-632's: the five
absolute read-only roots of `research/penpot-light`, `--entry app.main
--optimize none --no-cache --stats --stats-file FILE`, every budget from
docs/native-macros.md (`--macro-memory-limit 4000000000`) and
`--source-map FILE --source-map-include-sources`. Two peers (light-cljs-646
and the coverage script) built now and then; the 1-minute load average
before each run is recorded. Before and after alternated, three rounds;
medians [range]. Peak memory is `/usr/bin/time -l`'s maximum resident set
size; GB is 1e9 bytes.

| Build | Threads | Wall | User CPU | Peak memory | 1-min loads | Result |
| --- | --- | ---: | ---: | ---: | --- | --- |
| Pants, before | 1 | 86.9 s [86.2-92.8] | 85.9 s | 1.02 GB | 4.2, 3.2, 3.5 | stops at `core.MapEntry` |
| Pants, after | 1 | 66.5 s [63.1-67.6] | 65.3 s | 1.15 GB | 6.1, 2.8, 2.7 | same stop |
| Pants, before | 4 | 95.5 s [93.1-96.3] | 107.9 s | 1.22 GB | 4.8, 2.7, 3.8 | same stop |
| Pants, after | 4 | 60.6 s [59.9-62.2] | 72.5 s | 1.41 GB | 4.2, 2.5, 4.8 | same stop |
| V8 | 1 | 12.3 s [12.2-12.5] | 14.2 s | 1.10 GB | 2.3, 2.7, 3.0 | builds |
| V8 | 4 | 7.8 s [7.7-8.1] | 18.1 s | 1.42 GB | 2.7, 2.5, 2.4 | builds |

The V8 build compiles all 922 namespaces and emits; the Pants build stops
in namespace 727 before emission, so whole-build ratios understate the gap.
From the `--stats-file` traces (top-level pass plus deferred units per
namespace, one thread), on the 728 namespaces both builds reach:

| | Pants, before | Pants, after | V8 | Slowdown vs V8, after |
| --- | ---: | ---: | ---: | ---: |
| all 728 namespaces | 95.8 s (sampled run) | 61.9 s | 7.5 s | 8.3x |
| `instaparse.abnf` (the grammar macro) | 41.4 s | 14.1 s | 0.70 s | 20x |
| the other 727 | 54.4 s | 47.8 s | 6.8 s | 7.0x |
| `app.main.ui.icons` | 2.28 s | 1.90 s | 0.19 s | 9.9x |
| `app.main.ui.dashboard.team` | 1.76 s | 1.68 s | 0.13 s | 13x |
| `app.plugins.shape` | 0.86 s | 0.65 s | 0.16 s | 4.2x |

User-interface namespaces (rumext components) are 10-14x slower than V8,
the plugin proxies 4-6x. JavaScript commands and form conversions are 92%
of the one-thread wall time after the fix (`trace.top-level.engine-ms` +
`trace.unit.engine-ms` = 58.3 s of 63.0 s).

### 1. The grammar macro and the quadratic credit (fixed)

`precompile-cljs-grammar` runs instaparse's parser over its grammar text at
macro time. On the Pants build that one call ran 297 collections (24.9-27.7 s
by the pause counter) and admitted 514.6 GB to the byte budget; the whole
rest of the build admitted 31 GB. The admissions grew linearly in size, from
223 KB to 3.2 MB each (a temporary backtrace of every admission over 64 KB
put them all in `JsString::concat_accounted` under the interpreter's `+`):
a loop appending to one string. Since light-cljs-632 a rope keeps a credit
for its possible flattened copy only while a handle can still flatten it, so
used bytes stay linear; but every credit still counted as allocation volume,
and the byte trigger collects the whole heap after half the remaining
headroom has been admitted. With a 4 GB limit and 200-400 MB live, that was a
full collection every ~1.9 GB of credits, each tracing the whole heap.

The fix: a credit is reserved against the limit without counting as volume
(`ByteBudget::reserve_credit`, crates/big-boy-pants-js/src/heap/budget.rs)
and counts once a flatten makes the copy (`ByteReservation::allocated`).
The limit is enforced exactly as before. The regression test
`an_append_loop_admits_volume_linear_in_its_length` (src/value/string.rs)
admitted 401 MB for 2,000 appends of 100 units before the fix and fails;
it now admits about 500 KB. On Penpot the grammar macro falls from
37.3-39.6 s to 14.1-14.6 s at 1 thread and from 297 to 48 collections;
the build's admitted volume from 545 GB to 34 GB, its collections from 880
to 631. Peak memory rises 13-15% (fewer collections leave more garbage
between them). `app.render` from the fixed Pants build is byte-identical to
the V8 build's, JavaScript and source map (SHA-256 `0cc8853b…` and
`2f369231…` with this run's output paths).

The same search found a second, smaller accounting cost that did not matter
here: the regular-expression matcher charges 256 bytes of volume per
matching step (`regexp::matcher::Budget::charge`), so a failing `exec` over
a 7,500-unit string admits 3.9 MB. Charging the backtracking stacks' high
water instead cut a synthetic macro of 20,000 such searches from 2.4 s to
1.0 s, but did not change Penpot (regular expressions are 0.3% of its
samples), so it is not part of this change.

`--stats` and `--stats-file` now also report a failed build's counters up
to the failure (src/main.rs, src/compiler.rs); before, a build that stops,
as `app.main` does, printed none. `stats_report_a_failed_build_up_to_its_failure`
(tests/cli.rs) covers it.

### 2. Where the main thread's time goes, one thread

Share of the main thread's samples (`/usr/bin/sample`, 1 ms, whole build;
`bb benchmarks/sample-inclusive.bb SAMPLE --buckets ...`, each sample given
to the innermost listed function on its stack). Sampling slows the build
(97 s and 81 s), so use shares, not times:

| Work | Before | After |
| --- | ---: | ---: |
| interpreting JavaScript (`execute_budgeted`) | 47.5% | 65.0% |
| garbage collection (`Heap::mark_and_sweep`) | 39.7% | 18.8% |
| Rust analysis (`light_cljs::analyzer`) | 2.9% | 4.1% |
| task command loop (`BbpEvaluator::run`'s own work) | 3.7% | 3.5% |
| compiling JavaScript source on a helper thread (join) | 2.1% | 3.1% |
| form conversion (import, export, `&env`) | 1.8% | 2.6% |
| preparing and emitting deferred units | 1.6% | 2.2% |
| prune walk | 0.2% | 0.3% |
| everything else | 0.5% | 0.6% |

Form conversion, the prune walk, catalog application (`native.catalog.ms`
377 ms) and helper-producer setup (45 ms) are small on Penpot. The
single-entry `&env` cache never hits (`native.env.hits` 0 of 14,284;
485 ms), which is under 1%.

Inside the interpreter (inclusive shares of the main thread, after): the
dispatch loop's own time 16.4%, form intrinsics (`form_step`) 8.9%, calls
(`call`, `call_slow`, constructor entry) about 12%, property reads
(`lookup_cursor`, `property_read_step`, `read_named`) about 14%, dropping
values 3.7%, compiling `eval` source 2.5%.

### 3. What the interpreter runs: the port against what survives

`/usr/bin/sample` sees Rust frames only. To split interpreter work by the
JavaScript function running, a temporary build (not committed) counted
every 64th dispatch-loop iteration under the executing function's script,
name and first line. Iterations approximate instructions, not time; the
native work an instruction does is charged to its function. Groups by the
line ranges of the assembled `src/bbp_runtime.js` (its `decl`, `native` and
root sections):

| Code | Before the grammar macro | The grammar macro | Whole run |
| --- | ---: | ---: | ---: |
| the port's core functions and helpers (`native` section) | 50.3% | 42.7% | 48.1% |
| compiled macro and library code (helpers, `eval` units) | 27.0% | 33.7% | 29.0% |
| protocol and type dispatch (`decl`) | 13.4% | 13.6% | 13.4% |
| compiler plumbing (roots, initializers, command loop) | 9.3% | 9.9% | 9.5% |

The largest single port functions: `str` (13.1% before the grammar macro,
its inner mapping function included), `invoke` (6.4%), the macro-function
wrapper `macroReaderFunction` (3.5%), `val` (2.5%); in the grammar macro,
JavaScript interop dispatch (`jsMember` and friends, 8.7%), `coreLookup`
(5.4%), `invoke` (4.9%) and atom operations with their lane checks (`lane`,
`swapPair`, `resetPair`, `deref`, `notifyWatches`, about 10%). These belong
to the port and are not optimized here: the real core replaces them with
`src/js_runtime.js` definitions that run in the same interpreter, so their
share moves rather than disappears, and light-cljs-652 measures by how
much. Everything else (compiled macro code, dispatch over engine tables,
plumbing, and the interpreter's cost per instruction, call and property
read underneath all of it) survives the switch.

### 4. Garbage collection after the fix

The 631 collections left at 1 thread were not counted by trigger. Dividing
34 GB admitted by ~1.9 GB of byte headroom gives about 18 as a rough upper
bound on byte-triggered collections under that headroom assumption. Every
collection, including one from the allocation-count trigger, resets progress
toward the byte threshold, so the actual byte-triggered count could be lower,
including zero. The allocation-count trigger collects after as many object
allocations as there are live objects (at least 16,384). Penpot keeps only 65,000-72,000
live objects, but most live bytes sit inside persistent values (forms,
vectors, compiled code) that every collection traces in full, and those
grow with the build. Per 2,000 macro calls (pause counter, which undercounts
collection work as 575 found; samples put collection at 18.8%):

| Macro calls | Collections | Pause per collection | Live bytes at the end |
| --- | ---: | ---: | ---: |
| 0-2,000 | 90 | 5.3 ms | 88 MB |
| 2,000-4,000 | 131 | 10.5 ms | 112 MB |
| 8,000-10,000 | 88 | 15.7 ms | 164 MB |
| 12,000-14,000 | 68 | 17.9 ms | 174 MB |
| 14,000-14,288 (grammar macro) | 54 | 49 ms | 412 MB |

Collections do not get rarer as each one costs more: collection time grows
with the program's size times its allocations, faster than linearly. This
is the second surviving cause (light-cljs-664).

### 5. Why 4 threads was slower than 1, and is only 1.1x faster now

At 4 threads the grammar macro runs on a pool worker while the coordinator
goes on with later namespaces until one of its tasks touches a compile-time
atom. An effect lane opens only when every unit and pass segment scheduled
before the task has finished (src/effect_lanes.rs), whether or not it
touches that atom, so the coordinator waited for the whole grammar macro:

| 4 threads | Before | After |
| --- | ---: | ---: |
| longest lane wait | 48.2-49.0 s | 14.8-15.2 s |
| lane waits, summed over tasks | 86-90 s | 54-56 s |
| the grammar macro on its worker | 48.2-49.1 s | 14.8-15.2 s |
| its collections, pause, live bytes | 320, 34.9 s, 719 MB | 28, 2.3 s, 918 MB |
| coordinator's top-level passes (`trace.top-level-ms`) | 80.7 s | 47.8 s |
| deferred units, summed (`trace.units.host-ms`) | 120.0 s | 87.7 s |

Before the fix, the macro was slower on a worker (48-49 s) than on one
thread (37-40 s): with more units in flight the live heap was twice as
large (719 MB against 377 MB), so each of its credit-paced collections
traced twice as much, and every collection stops every thread. The
coordinator sat in the lane for that whole time, so the build's critical
path was the work before the macro, plus the slower macro, plus the rest.

After the fix the lane wait still equals the macro's duration. In the
samples of a four-thread run (77 s sampled), the coordinator is blocked 66%
of the time: 45% in `BbpEvaluator::demand` (lane and helper waits) and 20%
in `SharedCatalog::claim_help` (waiting for units other threads analyze at a
drain); it interprets 22% of the time. The three workers are idle in the
scheduler 35-60% of the time; the `light-cljs-units` thread is idle
throughout. User CPU is 72 s for 61 s of wall time, about 1.2 cores. The
summed unit time (87.7 s against 43.2 s at 1 thread) is waiting, not
computing: user CPU rose only 7 s.

### Ranked causes that survive the switch to the real core

| Rank | Cause | Share | Pellet |
| --- | --- | --- | --- |
| 1 | Interpreter cost per instruction, call and property read | 65% of the one-thread main thread; 52% of interpreted work is code that survives, the rest moves to the real core's definitions | light-cljs-663 |
| 2 | Collections paced by object count while each traces a growing heap of persistent values | 19% of the one-thread main thread, growing with program size | light-cljs-664 |
| 3 | Effect lanes wait for every earlier unit, not only those touching the atom | longest wait 15 s of 61 s at 4 threads (the whole grammar macro); coordinator blocked 66% | light-cljs-665 |
| - | Drain waits (`claim_help`, 20% of the coordinator at 4 threads) | below the lane waits; already covered | light-cljs-578, -577, -573 |
| fixed | Rope credits counted as allocation volume | 25-28 s of collection pauses in an 87 s build at 1 thread, 35 s at 4 | this change |

Form conversion (2.6%), the command loop's own work (3.5%), compiling
JavaScript source (3.1%) and Rust analysis (4.1%) are each under 10%.

### Method and reproduction

Run from a worktree, with `S` a scratch directory and `P` the absolute
`research/penpot-light` path:

```sh
CARGO_BUILD_JOBS=4 cargo build --locked --release --no-default-features --features bbp-macros
/usr/bin/time -l target/release/light-cljs --compile --esm --entry app.main \
  --source-root $P/frontend/src --source-root $P/common/src --source-root $P/vendor \
  --source-root $P/generated --source-root $P/shims \
  --optimize none --jobs 1 --no-cache --stats --stats-file $S/trace.tsv \
  --macro-expansions 100000000 --macro-nodes 4000000000 \
  --macro-timeout-ms 3600000 --macro-memory-limit 4000000000 \
  --macro-poll-budget 1000000000000 \
  --source-map $S/out.map --source-map-include-sources > $S/out.js
```

Per-namespace times come from the trace's `namespace` rows (top-level
microseconds) plus its `unit` rows by namespace; per-call collection
counters from the `macro.gc.N.*` lines of `--stats`. Samples:
`sample PID 90 1 -mayDie -file $S/run.sample` started one second into the
run, then `bb benchmarks/sample-inclusive.bb $S/run.sample --buckets ...`
(`--thread N` for the workers). The JavaScript-function counts used a
temporary counter at the top of `execute_budgeted`'s loop keyed by the
executing code's script name, source length, function name and first line;
it is not in the tree.

## Re-measured against shadow-cljs, cached and uncached (October 29, 2026, at `0ad2514f`)

**Summary.** On Penpot, light-cljs at its default (`--jobs 1`) builds the dev
build in 0.39x shadow-cljs's wall time (2.5x faster) at 0.38x its peak memory,
and the release build in 0.25x the time (4.0x faster) at 0.28x the peak
memory, with 0.77x the gzip size (23% smaller). With each compiler's cache
populated and nothing changed, light takes 0.13x shadow's time for the dev
build and 0.04x for the release build (1.4 s against 11.1 s and 36 s); after
one leaf edit it takes 0.49x and 0.19x. Since the October 23 section light's
output is byte-identical, its uncached wall time is 3-5% lower and its peak
memory about 6% higher; shadow ran 3-8% faster than on October 23, so the
uncached ratios barely moved (dev 2.49x to 2.54x faster, release 4.23x to
4.01x). The October 23 section measured no cached builds, so the cached
ratios are new. The native macro backend still cannot build `app.main`; it
builds `app.render` with output identical to the default backend's.

Setup. light-cljs master `0ad2514f`, one release binary (`cargo build --locked
--release`, default features, V8 from the machine cache, thin LTO, one codegen
unit) and one native-only release binary (`--no-default-features --features
bbp-macros`), each copied to an immutable path. shadow-cljs 3.5.3 with the
Penpot frontend's own JVM options (no `-Xmx`, so the JVM default, as in every
earlier Penpot run). Apple M3 Ultra (Virtual), 24 logical CPUs, 32 GiB, macOS
15.7.3, OpenJDK 25.0.2, Clojure CLI 1.12.4.1618, Babashka 1.12.218, Node
24.22.1, Rust/Cargo 1.99.1 (Homebrew). The machine was reserved: one job at a
time, nothing else running. The 1-minute load average before each round was
1.7-7.7; shadow's JVM drives it up and the average lags, so the light builds
right after a shadow job start at 4-6. The port is unchanged (996 files,
`b36daf80...`, the October 17 fingerprint) and was not regenerated. Peak
memory is the maximum resident set size from `/usr/bin/time -l`; GB is 1e9
bytes. Medians of three rounds (range).

Kits: `benchmarks/penpot.bb` with `LIGHT_CLJS_EXTRA_ARGS="--stats"` (as on
October 23) for the uncached light builds and every shadow build;
`benchmarks/penpot_cache.bb DIR 3 warm leaf advanced advanced-leaf` for light's
cached builds. Two minimal kit changes made the cached and edited builds
possible without touching the read-only research checkouts: `penpot.bb` gained
`shadow-compile-warm` and `shadow-release-warm` (a shadow job that keeps
`.shadow-cljs` from the job before it) and `PENPOT_PORT`/`PENPOT_FRONTEND`
overrides; `penpot_cache.bb` gained `PENPOT_PORT` and an `advanced-leaf`
scenario. Shadow ran on an APFS clone of `research/penpot/{frontend,common}`,
light's cached and edited builds on an APFS clone of `research/penpot-light`;
the edit is `penpot_cache.bb`'s leaf edit (a comment appended to
`app.main.ui.workspace.palette`, which has one consumer), made the same way
to the same file in shadow's clone and restored after each round. Light's
uncached builds read the original port, so their output can be compared with
earlier hashes; the clone's longer root paths make its output 156 bytes larger.
Pellets light-cljs-563, 564 and 565 were still open; the user chose to
measure master `0ad2514f` now rather than after them.

| Build | Wall | User CPU | Peak memory | light/shadow wall, peak memory |
| --- | --- | ---: | ---: | --- |
| shadow `compile main` (dev), cold | 30.22 s (30.01-30.44) | 93.7 s | 2.275 GB | |
| shadow `compile main`, warm, no change | 11.14 s (11.10-11.23) | 43.0 s | 2.104 GB | |
| shadow `compile main`, warm, leaf edit | 11.16 s (11.00-11.18) | 44.8 s | 1.996 GB | |
| light `main --optimize none`, `--no-cache` | 11.90 s (11.86-12.42) | 13.9 s | 0.873 GB | 0.39, 0.38 |
| light `main --optimize none`, populating the cache | 14.42 s (14.29-14.43) | 16.0 s | 0.893 GB | |
| light `main --optimize none`, warm, no change | 1.42 s (1.37-1.49) | 1.3 s | 0.570 GB | 0.13, 0.27 |
| light `main --optimize none`, warm, leaf edit | 5.49 s (5.43-5.62) | 5.7 s | 0.852 GB | 0.49, 0.43 |
| shadow `release main`, cold | 55.99 s (55.22-56.69) | 144.2 s | 3.939 GB | |
| shadow `release main`, warm, no change | 35.94 s (35.79-36.14) | 106.3 s | 3.849 GB | |
| shadow `release main`, warm, leaf edit | 36.99 s (35.58-37.22) | 103.1 s | 3.720 GB | |
| light `main` advanced, `--no-cache` | 13.95 s (13.85-13.98) | 16.0 s | 1.095 GB | 0.25, 0.28 |
| light `main` advanced, populating the cache | 16.14 s (16.01-16.18) | 17.7 s | 1.107 GB | |
| light `main` advanced, warm, no change | 1.36 s (1.32-1.36) | 1.2 s | 0.542 GB | 0.04, 0.14 |
| light `main` advanced, warm, leaf edit | 7.10 s (7.08-7.11) | 7.7 s | 0.994 GB | 0.19, 0.27 |

Shadow reported `1209 files, 1189 compiled` cold and `5 compiled` warm with no
change for `compile` (7 after the edit), and `1176 files, 916 compiled` cold,
`0 compiled` warm and `2 compiled` after the edit for `release`: a warm
release still runs Closure over the whole program, about 32 s of its 36 s.
Light's warm build hits all 922 namespaces and reuses its stored output; the
leaf edit misses 153 of 922 (the edited namespace and everything compiled
after it that the coarse macro-dependency fallback ties to it). Populating the
cache costs 21% (dev) and 16% (advanced) over an uncached build.

Output size, raw bytes / `gzip -9` bytes, both release outputs with
pseudo-names (neither is a production size), dev totals over every emitted
JavaScript file:

| Build | light | shadow | light/shadow raw, gzip |
| --- | --- | --- | --- |
| release / advanced | 24,646,495 / 2,741,041 | 32,917,432 / 3,560,980 (10 modules) | 0.75, 0.77 |
| dev | 33,636,141 / 3,132,605 (one file) | 34,376,171 / 3,908,977 (1,220 files) | 0.98, 0.80 |

Shadow's sizes vary by a few hundred bytes between rounds; light's do not.
The scope difference recorded on October 23 still holds: shadow's `:main`
build also builds `render`, `rasterizer` and `util-highlight`, and light's
`app.main` includes the app's five lazy modules.

Other light configurations, uncached, default backend (three rounds each,
same kit and flags plus the ones named, `--macro-memory-limit` last wins):

| Build | `--jobs 4`, 1 GiB | `--jobs 8`, 2 GiB |
| --- | --- | --- |
| `main --optimize none` | 7.26 s (7.26-7.39), 18.3 s user, 1.112 GB | 7.32 s (7.30-7.43), 21.7 s user, 1.421 GB |
| `main` advanced | 9.46 s (9.33-9.57), 20.4 s user, 1.351 GB | 9.41 s (9.40-9.50), 23.7 s user, 1.721 GB |

Against shadow these are 0.24x (dev) and 0.17x (release) the wall time. Eight
jobs are no faster than four, as on October 23, so this is not evidence of
many-core scaling: the build's remaining serial work bounds it.

Native macro backend (native-only binary, `--stats`, one run each):

| Build | Jobs | Result | Wall | User CPU | Peak memory |
| --- | ---: | --- | ---: | ---: | ---: |
| `main --optimize none` | 1 | fails | 94.20 s | 90.4 s | 0.701 GB |
| `main` advanced | 1 | fails | 93.38 s | 89.9 s | 0.705 GB |
| `main --optimize none` | 4 | fails | 53.70 s | 63.8 s | 0.884 GB |
| `app.render --optimize none` | 1 | builds | 13.33 s | 12.8 s | 0.411 GB |
| `app.render --optimize none` | 4 | builds | 9.00 s | 11.7 s | 0.509 GB |

All three `app.main` runs stop at the same first failure (task number 7 at
four jobs):

```text
light-cljs: .../research/penpot-light/vendor/instaparse/abnf.cljc:129:21: macro expansion: while expanding precompile-cljs-grammar: .../vendor/instaparse/abnf.cljc:129:21: macro instaparse.abnf/precompile-cljs-grammar: big-boy-pants-js: Atom(NotShareable): atom operation: cannot publish: value[0].capture(1)[0]{:regexp}{:parsers}[0]{:regexp} is a task-owned object (owned by task#1), which is not shareable (in anonymous at instruction 9 [atom_cas @ byte 23], task#1, world#0) at source 1126:33
```

This is instaparse's regular-expression parser stored in a compile-time atom,
the boundary light-cljs-563 (shareable regex values) addresses; the brief for
this run expected the later mutable-field `deftype` boundary of light-cljs-564,
which is reached only on the unmerged light-cljs-558/563 branches. Both
`app.render` outputs are 12,680,976 bytes with SHA-256 `d352af3a2bb2d35f...`,
the default backend's `app.render` hash of October 23: the native backend's
output is now byte-identical to the default backend's on this entry (it
differed in 47 `reify` IDs and 16 property orders at `f7f9aef0`, docs/native-macros.md).

Correctness, coverage and performance, separately:

- *Correctness.* Every uncached default-backend output, at `--jobs` 1, 4 and
  8, has the October 23 SHA-256 (`main-none` `dbf5cfcf91e48aaa`,
  `main-advanced` `fe303824959337ac`). On the clone, the populating, warm and
  leaf-edited builds' outputs equal the uncached build's of the same root
  (`cebf1e2f2c14501a` dev, `9b1963dcd0986415` advanced; the edit is a
  comment). Penpot was not bundled or run in a browser.
- *Coverage.* `app.main` builds at every default-backend configuration;
  natively only `app.render` builds (above).
- *Performance.* Light is faster and smaller in peak memory than shadow in
  every row. The largest differences are in cached builds, where shadow's
  release build re-runs Closure; the smallest is the dev leaf edit (0.49x),
  where light recompiles 153 namespaces for a one-namespace change.

Against the October 23 section (`81f9837`), the same binary flags:

| Build | October 23 | October 29 | change |
| --- | --- | --- | --- |
| light `main-none`, `--jobs 1` | 12.58 s, 0.819 GB | 11.90 s, 0.873 GB | -5.4% wall, +6.6% peak memory |
| light `main-advanced`, `--jobs 1` | 14.41 s, 1.030 GB | 13.95 s, 1.095 GB | -3.2%, +6.3% |
| light `main-none`, `--jobs 4` | 7.48 s, 1.095 GB | 7.26 s, 1.112 GB | -2.9%, +1.6% |
| light `main-advanced`, `--jobs 4` | 9.73 s, 1.357 GB | 9.46 s, 1.351 GB | -2.8%, -0.4% |
| light `main-none`, `--jobs 8` | 7.46 s, 1.432 GB | 7.32 s, 1.421 GB | -1.9%, -0.8% |
| light `main-advanced`, `--jobs 8` | 9.81 s, 1.712 GB | 9.41 s, 1.721 GB | -4.1%, +0.5% |
| shadow `compile` | 31.30 s, 2.210 GB | 30.22 s, 2.275 GB | -3.5%, +2.9% |
| shadow `release` | 60.93 s, 4.225 GB | 55.99 s, 3.939 GB | -8.1%, -6.8% |

Conclusions: nothing regressed in wall time; the one-job peak memory rose
about 6% (0.82 to 0.87 GB dev, consistent across rounds; the October 27
repair measured 0.85-0.87 GB with `--stats`), which is small and was not
investigated here. Shadow's own 3-8% improvement against October 23 is
machine variation (same version and options), which is why the ratios rather
than light's absolute times are the comparison to quote. The default
backend's Penpot numbers are where they were after the October 27 repair; the
work since then (the native backend) shows here only as the native rows.

## Re-measured against shadow-cljs (October 23, 2026, at `81f9837`)

light-cljs master `81f9837` (after light-cljs-426 to 429, 432 to 437, 439,
441, 442 and 459) against fresh shadow-cljs 3.5.3, back to back on the same
machine in one session (24-core Mac, macOS 15.7.3, 32 GB, OpenJDK 25.0.2),
with no other job running. `bb benchmarks/penpot.bb DIR shadow-compile
shadow-release` and then `render main-none main-advanced` for each
light-cljs configuration, `LIGHT_CLJS_BINARY` an immutable copy of the
release binary and the configuration in `LIGHT_CLJS_EXTRA_ARGS` (with
`--stats`), 3 rounds with the light-cljs configurations in reverse order in
round 2. Medians (range). Peak RSS from `/usr/bin/time -l`. The 1-minute
load average was 2.3 to 12.4: shadow's JVM drives it to 9-12 while it
runs, and the average lags, so the light-cljs builds right after a shadow
job record 5-10 although the machine was otherwise idle.

Configurations: *today* is today's default, `--jobs 1 --macro-memory-limit
67108864`; *proposed* is light-cljs-434's proposed default, `--jobs 4
--macro-memory-limit 1073741824` (256 MiB per job); *8 jobs* is `--jobs 8
--macro-memory-limit 2147483648`. These flags come after `penpot.bb`'s own
`--macro-memory-limit 4000000000`, and the last one wins.

| Build | Wall | User CPU | Peak RSS | vs shadow (wall, RSS) |
| --- | --- | ---: | ---: | --- |
| shadow `compile main` (dev) | 31.30 s (30.91-34.68) | 90.6 s | 2.210 GB | |
| light `main --optimize none`, today | 12.58 s (12.13-12.95) | 15.3 s | 0.819 GB | 2.49x faster, 0.37x |
| light `main --optimize none`, proposed | 7.48 s (7.33-7.49) | 18.4 s | 1.095 GB | 4.18x faster, 0.50x |
| light `main --optimize none`, 8 jobs | 7.46 s (7.44-7.71) | 22.0 s | 1.432 GB | 4.20x faster, 0.65x |
| shadow `release main` | 60.93 s (56.02-65.57) | 145.6 s | 4.225 GB | |
| light `main` advanced, today | 14.41 s (14.19-15.35) | 17.1 s | 1.030 GB | 4.23x faster, 0.24x |
| light `main` advanced, proposed | 9.73 s (9.50-9.87) | 20.7 s | 1.357 GB | 6.26x faster, 0.32x |
| light `main` advanced, 8 jobs | 9.81 s (9.27-9.98) | 24.3 s | 1.712 GB | 6.21x faster, 0.41x |
| light `app.render --optimize none`, today | 2.21 s (2.13-2.31) | 2.6 s | 0.403 GB | |
| light `app.render --optimize none`, proposed | 1.32 s (1.29-1.36) | 3.5 s | 0.578 GB | |

Shadow reported `1209 files, 1189 compiled, 0 warnings` for `compile` and
`1176 files, 916 compiled, 0 warnings` for `release`. Its `:main` build is
wider than light's `app.main`: it also builds the `render`, `rasterizer` and
`util-highlight` modules and writes source maps. Of its release output
(32.9 MB over ten module files, pseudo-names on), those three modules'
own files are 0.27 MB (0.8%; code they share with `main` is in `shared.js`),
and light-cljs builds `app.render` alone in 2.21 s today and 1.32 s
proposed, so the scope difference is worth at most a few seconds of
shadow's 31 to 61 s. light's `main` includes the app's five lazy modules.
Every light-cljs output is one SHA-256 per build at every configuration and
at both commits below (first 16 hex digits: `main-none` `dbf5cfcf91e48aaa`,
`main-advanced` `fe303824959337ac`, `render` `d352af3a2bb2d35f`).

Against the parent of light-cljs-426 (`dd1c1be`), built as a second
immutable binary and alternated with master in the same rounds:

| Build | `dd1c1be` | `81f9837` | change |
| --- | --- | --- | --- |
| `main-none`, `--jobs 1`, 64 MiB | 20.48 s, 25.9 s user, 0.826 GB | 12.58 s, 15.3 s user, 0.819 GB | -38.6% |
| `main-none`, `--jobs 8`, 2 GiB | 10.04 s, 33.3 s user, 1.443 GB | 7.46 s, 22.0 s user, 1.432 GB | -25.7% |
| `main-none`, `--jobs 8`, 64 MiB | fails 3 of 3 | 12.01 s, 16.4 s user, 0.858 GB | |
| `main-advanced`, `--jobs 1`, 64 MiB | 22.26 s, 27.4 s user, 1.075 GB | 14.41 s, 17.1 s user, 1.030 GB | -35.3% |
| `main-advanced`, `--jobs 8`, 2 GiB | 12.17 s, 35.3 s user, 1.700 GB | 9.81 s, 24.3 s user, 1.712 GB | -19.4% |

At `dd1c1be`, eight jobs at 64 MiB fail in `instaparse.abnf/precompile-cljs-grammar`
("JavaScript session memory limit exhausted"); at `81f9837` the slots beyond
the isolates get engine-less workers (light-cljs-437), so the build succeeds
with no worker isolate. `--stats` medians for `main-none` at `--jobs 1`:
V8 time (`trace.engine-ms`) 12.90 s to 6.55 s, of which deferred units 9.53
to 3.79 s; engine entries 1,319,396 to 548,210; units entering the macro
host 7,573 to 3,758 of 13,361; top-level passes 4.80 to 4.23 s. At eight
jobs 8,975 units need no engine at all (5,304 at `dd1c1be`), and the
coordinator's V8 time is its top-level passes' (2.81 s); its waits at drain
points are 1.44 s (1.97 s at `dd1c1be`).

With light-cljs-426's temporary macro probe (`--jobs 1`, 64 MiB, one run,
12.03 s with the probe against 24.2 s at `dd1c1be` on October 22): 300 core
macro calls still reach the engine (`..` 196, `as->` 58, `hash-map` 22,
`doto` 16, and 8 calls to `amap`, `areduce`, `comment`, `dotimes` and large `js-obj`), 0.06 s, against 28,090 and 6.27 s; the 20,126
calls to source macros take 7.89 s, 392 us per call, against 11.06 s and
549 us. The largest are `mf/defc` (1,033 calls, 2.93 s),
`precompile-cljs-grammar` (1 call, 0.73 s), `stl/css` (5,131, 0.52 s) and
`mf/html` (105, 0.49 s).

QuickJS-only (`--no-default-features --target-dir target/quickjs`) at the
proposed default, 3 rounds: `main-none` 31.94 s (31.61-32.61), 53.1 s user,
0.985 GB, the same output hash, against shadow `compile`'s 31.30 s. Its
coordinator spends 18.81 s in top-level passes (17.12 s of it in QuickJS)
and waits 11.44 s at drain points.

Output size, both with pseudo-names (so neither is a production size):
light `main-advanced` is 24,646,495 bytes and 2,741,041 with `gzip -9`;
shadow's release modules concatenated are 32,917,428 bytes and 3,561,142
(render and rasterizer, 36,779 of it), so light's is 23% smaller after
gzip.

## Earlier measurements

The entries below retain the state and decisions at each earlier phase.

Previous isolated V8 validation (phase111): **22.95 s / 0.967 GB RSS** for main
none, **28.81 s / 1.162 GB** for advanced, and **3.80 s / 0.479 GB** for render.
These use ordinary adaptive heap defaults, with no threshold override. All
1,442 tests, Clippy, formatting and three Node workflows pass; complete outputs
are unchanged. Native packaging and a final focused speed experiment are in
progress. [Phase111 evidence](results/compiler-2026-10-16/penpot-phase111/README.md).

Previous validated production rerun (phase101): **103.58 s / 0.803 GB RSS** for
main none, **108.10 s / 0.901 GB** for advanced and **11.47 s / 0.309 GB** for
render. Fresh shadow dev is **36.19 s / 2.331 GB**; release is **64.96 s /
4.345 GB**. The compiler now handles Rumext's native destructuring without
redeclaring its lexical locals. **All three complete outputs pass explicit
ES-module syntax checking.** This fixes the defect documented in phase44.

The exact promoted candidate passes **1,406 tests, zero failures, 106 ignored**
at normal concurrency, strict Clippy, formatting and all three Node workflows.
Regression coverage includes macro execution, every optimization level, mixed
raw/local bindings, rest/default destructuring, nested function shadowing,
strict directives, loops and captured iteration values. The port is unchanged.

The output audit finds only intended declaration changes: main none has 8,012
`const` declarations changed to lexical `let` for locals exposed to raw JS,
and 74 native `var` destructuring statements changed to assignments. Advanced
has 7,704 and 74 respectively; render has 409 and zero. All other lines match
phase100. This is syntax and focused semantic validation, not a bundled/browser
execution of Penpot. Production is **2.86 times slower** than fresh shadow dev,
at 34.5% of its RSS. The dramatic speed goal remains unmet.
[Phase101 evidence](results/compiler-2026-10-16/penpot-phase101/README.md) retains
the patch, full validation, output audits and all five benchmark runs.

Previous validated production rerun (phase100): **101.57 s / 0.802 GB RSS** for
main none, **105.90 s / 0.870 GB** for advanced and **11.13 s / 0.313 GB** for
render. Fresh shadow dev is **35.44 s / 2.188 GB**; release is **62.14 s /
4.468 GB**. After application assembly consumes a deferred unit's IR, the
session keeps generated source and immutable reader-registration tables instead
of retaining the full analyzed program. Macro execution remains lazy.

The exact promoted candidate passes **1,403 tests, zero failures, 106 ignored**
at normal concurrency, strict Clippy, formatting and all three Node workflows.
All three outputs remain byte-identical to phase42 and the port is unchanged.
Explicit ESM checking passes render and reproduces the known main defect below.

An immediate frozen-baseline comparison takes **102.86 s / 0.818 GB** for the
candidate versus **102.91 s / 0.892 GB** for phase87: 8.3% less RSS, with time
unchanged. The common drift from earlier measurements is not a patch regression.
Production remains **2.87 times slower** than fresh shadow dev, at 36.7% of its
RSS. The dramatic speed goal remains unmet. The faster V8 candidate below still
has unresolved runtime limits and remains unpromoted.
[Phase100 evidence](results/compiler-2026-10-16/penpot-phase100/README.md) retains
the implementation, validation, comparison and all five production benchmarks.

Previous validated production rerun (phase87): **94.02 s / 0.890 GB RSS** for
main none, **98.33 s / 1.051 GB** for advanced and **10.56 s / 0.352 GB** for
render. Fresh shadow dev is **31.17 s / 2.434 GB**; release is **56.86 s /
3.932 GB**. Private reader qualification now resolves bindings on demand.
Metadata-map layers and historical namespace tables share storage; completed
units release their boxed inputs and trim spare arena capacity.

This exact production candidate passes **1,401 tests, zero failures, 106 ignored**
at normal concurrency, strict Clippy, formatting and all three Node workflows.
All compiler, runtime, test, vendor and build inputs match the validated candidate.
The three complete outputs match phase42 and the 996-file port hash is unchanged.
Explicit ESM checking passes render and reproduces the known main defect below.

Main memory falls **11.8%** versus phase72. This batch does not improve production
time beyond run variation: main remains **3.02 times slower** than fresh shadow
dev, at 36.6% of its RSS. Advanced is 1.73 times slower than shadow release, at
26.7% of its RSS. The dramatic speed goal remains unmet. The faster V8 candidate
below still has unresolved runtime limits and remains unpromoted.

A native profile of this exact production binary (phase90) attributes **53.1%**
of main-thread observations exclusively to QuickJS's interpreter loop. JavaScript
macro expansion accounts for **87.5% inclusively**, including the bridge and
nested compiler work. These overlapping figures must not be added. The next
substantial speed gain needs to address macro execution; the retained storage
changes mainly improve memory.

Nested timing instrumentation (phase91) separates 48,216 JavaScript expansions:
54.29 seconds in macro calls, 7.35 in form import, 6.83 in result export, 6.46 in
environment import, 4.01 in argument preparation and 0.62 in root release.
Complete `rumext.v2/defc` expansions account for 21.33 seconds; one Instaparse
grammar expansion takes 12.37 seconds. Macro totals include transport and must
not be added to stage totals. The diagnostic run takes 91.16 seconds and produces
the identical output.

QuickJS safepoint sampling (phase92) finds repeated protocol selection and
private collection-store access alongside bridge traversal. `publicMethod`
appears in 32.6% of sampled stacks inclusively; its own top-frame share is 6.9%.
The declaration-demand callback contributes 7.8% of top frames; private-store
`has`/`get` wrappers contribute about 5.6%/3.9%. Safepoint sampling can miss
native work and merges anonymous functions with the same declaration line, so
these are targeting evidence rather than precise wall-time percentages.

Whole-build follow-ups do not turn those samples into useful further gains:
a ready-root shortcut takes **91.43 s / 0.916 GB**; a shorter protocol-demand
path takes **90.54 s / 0.908 GB**; native private-store methods take
**89.13 s / 0.917 GB**, against phase87's isolated **91.34 s / 0.894 GB**.
All are rejected, without full-suite runs. The demand-path regression passes;
complete application output is unchanged, with differences confined to the two
deliberately edited runtime units where applicable. The store trial also lacks
a justification for changing the runtime's established V8 allocation strategy.
Phases93–95 preserve the negative results. A stale instrumented-engine run in
phase93 is explicitly excluded; subsequent builds force the vendored engine
inputs and check the frozen binary for the diagnostic marker.

Previous validated production rerun (phase72): **92.72 s / 1.008 GB RSS** for
main none, **98.37 s / 1.153 GB** for advanced and **10.60 s / 0.397 GB** for
render. Fresh shadow dev is **30.19 s / 2.377 GB**; release is **56.55 s /
3.798 GB**. Unit runtime parameters now use the existing raw-JavaScript hygiene
pass, removing the scan through all previous string literals. This is a small
production gain (1.2% for main), not a dramatic further speedup.

The exact promoted candidate passes **1,400 tests, zero failures, 106 ignored**,
strict Clippy, formatting and all three Node workflows. One obsolete internal
scan test was removed; the existing raw-JavaScript regression now checks the new
parameter and its hygiene prefix. All source, test, vendor, bench and build
inputs match the validated candidate. All three complete outputs match phase42,
and the 996-file port hash is unchanged. Explicit ESM checking again passes
render and fails both main outputs with the known defect below.

Production remains **3.07 times slower** than shadow dev, at 42.4% of its RSS;
advanced is 1.74 times slower than shadow release, at 30.4% of its RSS. This is
about 22 times faster than the original 2,057-second build, but the goal of
dramatically outperforming shadow on both speed and memory remains unmet.

Previous validated production rerun (phase52): **93.83 s / 0.993 GB RSS** for
main none, **99.31 s / 1.162 GB** for advanced, and **10.70 s / 0.401 GB** for
render. Fresh shadow dev is **30.17 s / 2.232 GB**; release is **55.06 s / 4.046 GB**.
Production now selects mimalloc in the native CLI and emits compact, equivalent
registration strings for compile-time units. Library allocator selection and
WebAssembly dependencies are unchanged. The candidate passes **1,401 tests,
zero failures, 106 ignored**, strict Clippy, formatting and all three Node
workflows; all compiler, runtime, test and vendor inputs were checked identical
after promotion. All complete outputs match phase42 and the port is unchanged.
The known main ESM defect below persists.

Compared with phase47, main is 15.6% faster and uses 20.6% less peak RSS.
It remains **3.11 times slower** than shadow dev, at 44.5% of its peak RSS;
advanced is **1.80 times slower** than shadow release, at 28.7% of its RSS.
The overall speed goal remains unmet. The bundled allocator also passed a
Linux ARM64 C probe using the host's existing GCC; the host has no Rust
toolchain, so this is not a Linux compiler build or suite result. Phase52
preserves the dependency lock, incremental patch, full implementation patch,
commands, test logs, source/output hashes, timing and Linux probe evidence.

The latest isolated V8 candidate (phase110) combines JavaScript heap and backing
storage in its session budget. A negative control retained roughly 43.1 MB under
a 32 MiB limit when each category was checked separately. GC now refreshes the
allocator's heap observation, and operation/interrupt checks collect unreachable
storage before rejecting a combined over-budget state. This is cooperative
storage accounting, not an exact process-RSS cap.

The full suite passes **1,440 tests, zero failures, 106 ignored**, strict Clippy,
formatting and all three Node workflows. Main none takes **23.52 s / 0.990 GB**,
advanced **29.68 s / 1.162 GB**, and render **3.86 s / 0.482 GB**. An immediate
phase109 main takes **23.46 s / 0.983 GB**, showing no material overhead. All
complete outputs are identical and pass ESM syntax checking; the port is
unchanged. The latest fresh Shadow comparison remains phase109's **33.08 s /
2.197 GB** dev and **60.08 s / 4.129 GB** release.

Without the experimental 256 MiB heap threshold, main takes **23.82 s / 1.294 GB**
with the same output. An ordinary adaptive collection threshold and reproducible
backend packaging are next; production remains phase101.
[Phase110 evidence](results/compiler-2026-10-16/penpot-phase110/README.md) retains
the combined-limit negative control, patch, full validation and measurements.

The preceding isolated V8 candidate (phase109) adds backing-store quotas to a
source-built, pinned engine. Fixed and resizable buffers, shared buffers, Wasm
memory, growth, shrinkage and release now share an atomic backing-store budget.
Thirteen previously successful over-budget cases now fail with the owned
memory-limit diagnostic. Garbage-collection retries, zero-filled growth,
coercion order, caught errors and backing stores outliving an isolate are covered.
Allocator callbacks do not reenter V8.

The candidate passes **1,435 tests, zero failures, 106 ignored**, strict Clippy,
formatting and all three Node workflows. Cold main none takes **24.19 s /
0.967 GB**, advanced **29.05 s / 1.185 GB**, and render **3.89 s / 0.488 GB**.
Fresh Shadow dev takes **33.08 s / 2.197 GB**; release **60.08 s / 4.129 GB**.
All complete outputs match phase108 byte-for-byte and pass ESM syntax checks;
the port remains unchanged.

An immediate old-engine main takes 22.80 s. Two further alternating comparisons
give candidate times of **22.99 / 22.93 s** and baseline times of **23.15 /
23.09 s**. The three-run medians are 22.99 and 23.09 seconds, so the quota patch
does not show a consistent timing penalty or gain. Main remains about 30.5%
faster than fresh Shadow dev at roughly 44% of its RSS. Heap and backing storage
still have separate budgets; their combined session limit and backend packaging
remain to be addressed. Production stays at phase101 and the dramatic speed
goal remains open. [Phase109 evidence](results/compiler-2026-10-16/penpot-phase109/README.md)
retains the engine/compiler patches, negative control, full validation and runs.

The preceding isolated V8 candidate (phase107) installs its memory callback before
Context creation and monitors JavaScript runtime initialization with the shared
interrupt scheduler. Failed startup cleans up the partial session and preserves
the owned resource diagnostic; scheduler thread creation returns an error and
can be retried. Native Isolate/Context creation remains synchronous, and the
conservative 16 MiB startup minimum remains. Backing-store memory accounting is
still unresolved, so the backend remains isolated.

The exact candidate passes **1,426 tests, zero failures, 106 ignored** at normal
concurrency, strict Clippy, formatting and all three Node workflows. Cold builds
take **22.74 s / 0.956 GB** for main none, **28.93 s / 1.158 GB** for advanced,
and **3.77 s / 0.507 GB** for render. Fresh Shadow dev takes **33.15 s / 2.593 GB**;
release takes **61.55 s / 4.810 GB**. All three outputs match corrected production
byte-for-byte and pass explicit ESM syntax checks; the port is unchanged. Main is
31.4% faster than Shadow dev at 36.9% of its RSS. The dramatic speed goal remains
unmet. [Phase107 evidence](results/compiler-2026-10-16/penpot-phase107/README.md)
retains the implementation, full checks and all five runs.

A pinned V8 source-build baseline (phase108) now passes the same **1,426 tests,
zero failures, 106 ignored**, strict Clippy, formatting and three Node workflows.
The published crate omitted ICU data and vendored Rust libraries; these were
restored from its exact upstream submodule revisions. Chromium's pinned native
toolchain builds the unmodified engine in 5 minutes 40 seconds. Frozen library
and generated-binding hashes tie validation to that exact build.

Main none takes **23.12 s / 0.986 GB**, versus an immediate frozen prebuilt-engine
rerun at **23.06 s / 0.967 GB**. Advanced takes **29.07 s / 1.166 GB** and render
**3.84 s / 0.486 GB**. All outputs match phase107 and pass explicit ESM checks.
This establishes the native baseline for the backing-store fix; it does not
implement that fix or promote V8.
[Phase108 evidence](results/compiler-2026-10-16/penpot-phase108/README.md) preserves
the pinned restoration steps, setup failures, build, full checks and measurements.

The preceding isolated V8 candidate (phase102) includes phase101's native
destructuring fix and passes **1,421 tests, zero failures, 106 ignored** at
normal concurrency, strict Clippy, formatting and all three Node workflows.
Clean cold builds take **24.21 s / 0.971 GB** for main none, **30.51 s /
1.180 GB** for advanced and **3.90 s / 0.479 GB** for render. All three outputs
are byte-identical to corrected production and pass explicit ESM syntax checks.
Main is 33.1% faster than fresh Shadow dev, using 41.6% of its RSS. V8 remains
isolated because of the runtime-limit and packaging gaps documented below.
[Phase102 evidence](results/compiler-2026-10-16/penpot-phase102/README.md) retains
the exact patch, tests, commands, hashes and measurements.

Lowering V8's JIT invocation thresholds tenfold (phase103) takes **23.71 s /
0.985 GB** for main versus an immediate default-flags rerun at **23.96 s /
0.949 GB**. The small wall-time difference comes with 13.7% more user CPU time
and 3.9% more RSS, so the setting is rejected. Complete outputs are unchanged;
the rejected flags did not receive a full-suite run.
[Phase103 evidence](results/compiler-2026-10-16/penpot-phase103/README.md) records
the exact settings and comparison.

A fresh native/JavaScript profile of phase102 (phase104) takes **24.87 s** and
produces the identical output. JavaScript bridge traversal accounts for 10.1%
of sampled self time, while encoding and decoding take 3.16 and 2.67 seconds
inclusively. Macro calls take 9.02 seconds inclusively; these overlapping totals
must not be added. Native samples also show repeated namespace-prefix counting
and validation of unread environments. The profile is targeting evidence, not
an uninstrumented benchmark; the native sampler itself appears in 7.7% of
main-thread observations.
[Phase104 evidence](results/compiler-2026-10-16/penpot-phase104/README.md) retains
both original profiles, summaries, source mapping and instrumentation.

A lazy error-path experiment (phases105–106) passes 1,400 differential cases but
does not produce a useful whole-build gain. V8 takes **22.51 s / 0.955 GB** versus
an immediate frozen baseline at **22.95 s / 0.963 GB**. QuickJS takes **96.12 s /
0.816 GB** versus **95.27 s / 0.798 GB**. The change is rejected; output differences
are confined to the deliberately edited runtime function and complete ESM syntax
checks pass. No full suite was run for these rejected candidates.
[Phase105](results/compiler-2026-10-16/penpot-phase105/README.md) and
[phase106](results/compiler-2026-10-16/penpot-phase106/README.md) preserve the probes
and sequential comparisons.

The preceding isolated V8 candidate (phase99) passes **1,418 tests, zero failures,
106 ignored at normal concurrency**, strict Clippy, formatting and all three
Node workflows. After application assembly consumes a deferred unit's analyzed
body, the session retains generated source and immutable reader-registration
tables instead of the full analyzed program. Macro execution and registration
remain lazy. Prepared source and registration match direct emission.

Clean cold builds take **22.87 s / 0.953 GB** for main none, **28.92 s / 1.120 GB**
for advanced and **3.70 s / 0.481 GB** for render. Main peak RSS falls 15.2% versus
phase97; time is essentially unchanged. All complete outputs match phase42,
including the known main ESM defect. Phase98's teardown measurements explain
the target: only 277 of 13,367 retained programs had been emitted, and freeing
those saved just 4.0 MB; freeing the still-lazy programs released another
205.5 MB of requested Rust allocations. These are ordered releases, not RSS.
[Phase98 evidence](results/compiler-2026-10-16/penpot-phase98/README.md) and
[phase99 evidence](results/compiler-2026-10-16/penpot-phase99/README.md) retain
the diagnostic, implementation and validation. The corresponding QuickJS change
was subsequently validated and promoted in phase100, described above.

The preceding isolated V8 candidate (phase97) passes **1,416 tests, zero failures,
106 ignored at normal concurrency**, strict Clippy, formatting and all three
Node workflows. It adds a shared interrupt scheduler (phase96) and enforces
native stack limits at outer engine entries while preserving the boundary
through nested compiler callbacks. Linux ARM64 and Windows x64 native probes
pass for the OS boundary helper; full V8/compiler testing remains on macOS.

Clean cold builds take **23.13 s / 1.124 GB** for main none, **29.30 s / 1.236 GB**
for advanced and **3.71 s / 0.514 GB** for render. An immediate comparison of the
frozen scheduler-only and bounded-stack binaries takes **22.64 versus 22.67 s**
for main, showing no meaningful stack-limit cost in that pair. Complete outputs
remain byte-identical to phase42; render passes ESM syntax checking and the main
defect persists. The port is unchanged. V8 remains isolated: backing-store
memory limits, startup monitoring, scheduler creation failure and packaging
still need work. The dramatic speed goal is
unmet. [Phase96 evidence](results/compiler-2026-10-16/penpot-phase96/README.md)
and [phase97 evidence](results/compiler-2026-10-16/penpot-phase97/README.md) retain
the patches, commands, tests, hashes, native probes and benchmark comparisons.

The preceding combined isolated V8 candidate (phase85) passes **1,409 tests, zero
failures, 106 ignored**, four test threads, strict Clippy, formatting and all
three Node workflows. Clean cold builds take **20.57 s / 1.089 GB** for main
none, **26.22 s / 1.232 GB** for advanced and **3.29 s / 0.522 GB** for render.
Shared map layers, smaller completed-unit entries, shared namespace tables and
trimmed arena capacity reduce clean main RSS by 12% versus phase76; time changes
by about 2%. All complete outputs match phase42, including its known ESM defect.
Main is 34% faster than phase87's fresh shadow dev and uses 45% of its RSS. The dramatic
speed goal remains unmet, and V8 remains unpromoted with the same runtime gaps.

The preceding combined isolated V8 candidate in phase76 passes **1,408 tests, zero
failures, 106 ignored**, four test threads, strict Clippy, formatting and all
three Node workflows. Clean cold builds take **20.98 s / 1.237 GB** for main
none, **26.36 s / 1.311 GB** for advanced and **3.40 s / 0.556 GB** for render.
This includes typed root registration and resolves reader bindings only when
syntax quotation reads them, removing private runtime binding-table construction
and declaration-kind scans. The public reader's binding introspection stays
unchanged. All complete outputs match phase42. Main is about 31% faster than
fresh shadow dev, at 52% of its RSS, but the dramatic speed goal remains unmet.
V8's resource-limit, watchdog and packaging gaps still prevent promotion.

Allocation diagnostics (phases77–79) separate churn from retention. At the end
of analysis, the phase76 prototype retains 648 MB of requested Rust allocations;
dropping its macro session releases about 451 MB. These counters exclude V8/C++
and allocator overhead. Sharing metadata-map layers removes **2.317 GB (15.2%)**
of cumulative allocation during analysis, but does not reduce that retained
state. The isolated layer-sharing build takes **20.31 s / 1.176 GB RSS** and
passes two focused snapshot tests; it has not passed a full suite. Complete
output remains identical to phase42. This is a copying improvement, not a fix
for the remaining session retention.

Teardown probes (phases81–82) find 13,367 completed units and no unfinished
definitions. Clearing their analyzed programs releases 314 MB, including about
183 MB from expression tables and 61 MB from namespace tables; clearing catalog
entries releases another 52 MB. These are releases in that order, not independent
object-size estimates. The phase85 changes address the namespace and empty-entry
costs. Shared entries within modified arena leaves (phase80) and reuse of exact
canonical bridge subtrees (phase86) show no useful gain and are not retained.

QuickJS namespace registration trials also fail to justify their complexity.
Splitting registration from unit code and caching compiled namespace functions
takes **89.04 s / 0.944 GB**; the same binary with caching disabled takes
**90.25 s / 0.947 GB**, versus the storage-only candidate's isolated
**91.34 s / 0.894 GB**. Four cache tests pass, including reentry, fresh namespace
state, restored source views, replaced eval and exceptions, but the timing change
is small and memory is worse. Neither trial is retained or given a full suite.
The exact outputs remain identical; phases88–89 preserve both results.

The preceding combined isolated V8 candidate in phase70 passes **1,406 tests, zero
failures, 106 ignored**, using four test threads, plus strict Clippy, formatting
and all three Node workflows. Clean cold builds take **22.59 s / 1.232 GB**
for main none, **28.13 s / 1.323 GB** for advanced and **3.69 s / 0.568 GB**
for render. All complete outputs match phase42, including the known main ESM
syntax defect. A 128 MiB V8 heap now completes in **22.92 s / 1.202 GB**;
the smaller managed heap only slightly reduces process memory. Main is 25%
faster than the latest shadow dev observation, but the dramatic speed goal is
still unmet. V8 remains unpromoted because its resource-limit, watchdog and
packaging gaps remain.

The preceding combined isolated V8 candidate passes **1,404 tests, zero failures,
106 ignored**, using four test threads, plus strict Clippy, formatting and the
Node workflows (phase58). It takes **25.69 s / 1.374 GB** for main none,
**30.89 s / 1.420 GB** for advanced and **4.15 s / 0.582 GB** for render, with
all complete outputs matching phase42. Its callback failures now retain owned
diagnostics without unchecked conversion/allocation unwraps or catch-and-spin
delays. Stack, backing-store, startup-limit, watchdog and packaging gaps remain;
V8 is still unpromoted. It is only 17% faster than shadow dev, so the dramatic
wall-time goal remains unmet.

The preceding thin-LTO observation was 25.14 s / 1.31 GB; small differences
between these runs are not evidence of a regression or a further gain. Range
iteration alone shows no timing gain. Compiler-runtime storage trials range
from 24.61 to 25.17 s and are not retained. A smaller 128 MiB V8 heap
fails during Instaparse expansion; that failed run is not a memory improvement.
Phases53–58 preserve the samples, patches, commands and negative results. One
shared-target Cargo artifact was stale; its measurement is explicitly excluded,
and subsequent isolated builds force compilation of both crate roots. None of
these trials changes the validated production result above.

The next isolated buffer guard (phase59) is **incomplete**. Fixed-size backing
storage tests pass, but growable SharedArrayBuffer bypasses both the custom
allocator and V8's external-memory total: a 33 MiB buffer succeeds under a
32 MiB limit. Its failing regression and counters are preserved. This confirms
that the faster backend's remaining resource-limit gap is real; the guard and
V8 backend are not promoted.

Further profiling and rejected trials (phases60–64): an in-process V8 CPU
profile identifies bridge traversal/snapshots, native deferred compilation,
generated-code evaluation and protocol dispatch as remaining costs. The
`forceSlot` samples are mostly its Rust callback, not JavaScript lookup itself.
Reusing macro argument traversal order gives 25.29 s on V8 and **94.49 s on
QuickJS**. Replacing active-ancestor weak sets with a bounded stack gives
25.09 s on V8 and **94.22 s on QuickJS**. Neither improves retained production,
so neither is promoted or put through a full suite. All output differences are
confined to the exact edited runtime function; the rest matches phase42.
The replay trial also passes 500 differential semantic cases. The original
profile, patches, exact commands, negative results and hashes are preserved.

Phases65–66 also test skipping `&form` transport for unwrapped core macros
whose formal is proven unread, with conservative metadata/literal/limit guards.
Ten focused bootstrap tests pass and complete Penpot output matches phase42.
Results are **24.82 s / 1.344 GB on V8** and **92.23 s / 1.012 GB on QuickJS**.
The production timing difference is only 1.7%, with slightly higher memory;
this does not justify the extra complexity, so the shortcut is not retained
and no full suite was run for it. Production remains the validated phase52.
An initial phase65 build reused a stale generated runtime despite rebuilding
Rust; that run is excluded. Scratch builds now force `build.rs` as well as
both crate roots, and the clean rerun verifies complete output identity.

The next measurements expose a larger duplication (phases67–69): **437 deferred
units emit 102.9 MB of namespace registration for only 0.92 MB of code**. A
bounded V8 cache compiles each namespace's latest registration function once,
while executing every registration in order with fresh namespace maps/sets.
It reduces compiled registration source to **1.05 MB** (131,239 hits / 1,292
misses). Diagnostic main-none runs, including terminal GC/statistics, take
**24.62 s / 1.173 GB** and **24.59 s / 1.177 GB** with batched execution.
All output matches phase42. Six focused V8 tests pass for the first cache,
including mutation, source-view restoration, eviction, replaced eval, reentry
and exceptions. Phase70 adds the full validation reported above and replaces
the whole-program string-table scan for unit parameter names with the existing
raw-JavaScript hygiene pass. Production and
the faster backend's remaining resource-limit gaps are unchanged.

Phase71 uses typed root registration in the isolated V8 evaluator, retaining
dynamic callback lookup and the replaced-eval fallback. It cuts normal generated
source evaluations from 14,977 to 993 and takes **22.40 s / 1.178 GB** in a
diagnostic run with terminal GC. Eight focused tests pass; phase76 subsequently
validates this delta in the full suite. Output matches phase42. Phase73's CPU profile and
phase74's native sample now put bridge traversal/snapshots, macro execution,
reader-view construction and global iteration ahead of namespace parsing.
Reader-view updates account for 882 of 9,130 inclusive native observations;
overlapping sample categories must not be summed. These are prototype findings,
not improvements to the production benchmark above.

The phase75 reader configuration revision trial takes **22.54 s / 1.240 GB**,
so it does not improve phase71's diagnostic run and is not retained. No full
suite was run for that rejected trial. Further phase74 sample attribution puts
386 of the 479 exclusive global-iterator observations inside reader-view updates;
phase76 resolves only the bindings syntax quotation actually reads and passes
the full suite reported above.

Further isolated trials, **not production changes**: selecting mimalloc for
the V8 CLI lowers main-none from 35.71 s to **26.51/26.58 s**, at **1.34/1.35 GB
RSS**, with identical complete output. This is faster than the latest shadow
dev observation, but the allocator variant has not passed a full suite and
the V8 resource-limit/portability gaps remain. The separate QuickJS quoting-only
trial took **104.20 s / 1.17 GB**, with identical output; phase52 validates and
retains it together with the native CLI allocator. The string-scan prefilter trial
takes 34.91 s in V8, a small difference requiring a repeat. Phases49–51 preserve
the isolated patches, exact dependency versions, commands and measurements.
The earlier validated production result is phase47 below.

Previous validated production rerun (phase47): **111.18 s / 1.25 GB RSS** for
main none, **116.56 s / 1.34 GB** for advanced, and **14.07 s / 0.50 GB** for
render. Fresh shadow dev is **30.27 s / 2.14 GB**; release is **56.36 s / 5.34 GB**.
Production passes **1,401 tests, zero failures, 106 ignored**, strict Clippy,
formatting, diff checks and all three standalone Node workflows. All complete
outputs match phase42 and the port is unchanged; the known main ESM defect
below persists.

ASCII compiler names now allocate their shared UTF-16 slice directly, avoiding
a temporary vector. The compiler's private reader view keeps only namespaces
that can affect name qualification: current, referred and aliased namespaces.
The regression preserves alias collisions, renamed refers, exclusions and
captured macro definition views, and passes against the old reader too.

Production main is 5.7% faster than phase45, with 1.8% higher peak RSS. It is
still **3.67 times slower** than shadow dev at 58% of its RSS; advanced is
**2.07 times slower** than shadow release at 25% of its RSS. The combined
isolated V8 candidate takes **35.71 s / 1.37 GB**, still 18% slower than shadow
dev. Its full suite passes **1,402 tests** with four test threads. An earlier
default-concurrency run hit two deadline-related failures, both passing alone;
that evidence remains recorded. V8 remains unpromoted. See phases46–48 for
patches, complete logs, allocation evidence and the fresh native sample.
The overall speed goal remains unmet.

Previous validated production rerun (phase45): **117.87 s / 1.23 GB RSS** for
main none, **123.01 s / 1.33 GB** for advanced, and **15.05 s / 0.51 GB** for
render. Fresh shadow dev is **30.90 s / 2.46 GB**; release is **57.24 s / 3.84 GB**.
Production passes **1,400 tests, zero failures, 106 ignored**, strict Clippy,
formatting and diff checks. Complete outputs match phase42; the unchanged
main ESM defect below prevents treating this as executable correctness.

Deferred analyzers now inherit the namespace-global count from their definition
snapshot, avoiding repeated scans of a frozen global prefix. Native sampling
identified that scan in 9.7% of main-thread observations in the isolated V8
compiler. Its clean main time falls from 47.32 s to **42.74 s / 1.44 GB**, and its
full suite passes 1,401 tests. Production's main time is essentially unchanged
from phase42; the V8 speedup is not evidence of a production speedup. V8 remains
unpromoted. Production is still 3.81 times slower than shadow dev, at 50% of its
RSS. Evidence is in `results/compiler-2026-10-16/penpot-phase45/`. The overall
speed goal remains unmet.

**Syntax-validation correction (phase44):** Earlier `node --check FILE.js`
commands returned zero, but that was insufficient for these ESM outputs.
Explicit `node --check --input-type=module < FILE.js` rejects the frozen
phase42 main-none output: rumext native destructuring emits `var` for locals
already declared `const`. The V8 prototype reproduces the same output exactly.
Earlier syntax-pass claims below are historical observations of the inadequate
check, not evidence that Penpot can execute. Byte equivalence and timing results
remain valid; bundling/browser correctness remains unestablished. Phase101
subsequently fixes this defect in the compiler, and all three new production
outputs pass the explicit ESM check. Historical outputs remain unchanged.

An **isolated, unpromoted V8 compiler prototype** (phases43–44) passes the full
suite with V8 selected for noninteractive compiler macros: **1,401 passed,
zero failed, 106 ignored**. The final clean runs are render **7.91 s / 0.65 GB**,
main none **47.32 s / 1.41 GB**, and advanced **53.84 s / 1.53 GB**, using a
256 MiB V8 heap limit. All complete outputs match phase42. Render passes an
explicit ESM syntax check; both main outputs fail as described above.

The prototype exposed another retention issue: V8 kept the whole script source
of macro functions, including large repeated namespace catalogs. Render's
heap snapshot contained 80 MB of strings out of about 90 MB live heap.
Separating registration from the function source reduces final live engine
heap to 11 MB for render and 27 MB for main. Compact compile-time string
quoting then reduces generated registration text. Disabling V8's compilation
cache did not meaningfully resolve retention. These live heap measures are
distinct from process peak RSS.

The prototype remains **1.56 times slower than shadow dev**, with 64% of its
peak RSS; it is 2.47 times faster than production, but uses 14% more peak RSS.
Its stack-limit support, bounded ArrayBuffer allocation, startup budgets,
watchdog behavior and exceptional registration reentry need work before
promotion. The production backend and dependency files remain unchanged. Evidence,
full test logs, failed heap trials, the reproducible patch and limitations are
in `results/compiler-2026-10-16/penpot-phase44/`; phase43 contains the standalone
engine and boundary probes. The requested overall goal remains unmet.

The prototype's scoped profile accounts for 14.61 s in top-level namespace
analysis, 10.19 s in deferred analysis, 7.69 s in macro calls, 9.57 s in
transport and 3.57 s in unit emission, with nested measured work subtracted.
Initial loading, final preparation and output together are below one second.
The namespace category includes source preparation and lazy loading, so native
profiling is needed to identify its expensive operations. Engine replacement
alone does not meet the speed goal.

Previous validated cold rerun (phase42): main none **117.05 s / 1.24 GB RSS**, main advanced **122.76 s / 1.30 GB**. Fresh shadow dev is **30.33 s / 2.22 GB**, release **55.72 s / 4.62 GB**.

Reader identity validation now resolves keyword/symbol names without inserting
unused entries into executable literal tables. Actual lowering uses the same
name-resolution helpers and still inserts its literals. Exact UTF-16 names,
auto-resolution and generated semantic names retain their existing paths.

A nine-key map previously retained 27 names and 27 strings; the regression
now requires nine of each. It also covers nested map/set keys, symbols and
auto-resolved keywords in source and owned compiler forms. The frozen old
library reproduces the 27/27 negative control.

Full suite: **1,400 passed, zero failed, 106 ignored**. Strict Clippy,
formatting and all three Node workflows pass. Render is **14.86 s / 0.49 GB**.
All three complete outputs match phase33 byte for byte; the port is unchanged.
Explicit ESM syntax checking fails for main-none as described above. Penpot
browser execution remains untested.

Main none is faster than phase33's 121.15 s observation, but its peak RSS
is higher than phase33's 1.17 GB. This run does not establish a whole-build
memory reduction; the regression checks demonstrate eliminated dead entries.

The separate live-allocation comparison of render confirms reduced retained
data: 312,381,712 to 290,492,640 bytes after analysis (7.0% less), and
104,108,000 to 94,240,688 bytes after releasing the macro session (9.5% less).
The reader-identity allocation category disappears; the classified QuickJS
allocation total is unchanged. These are live allocator sizes, not RSS.
The instrumented output also matches the validated output exactly.

None remains **3.86 times slower** than shadow dev at **56% of its peak RSS**. Advanced is **2.20 times slower** than shadow release at **28% of its RSS**. The requested speed goal remains unmet.

Evidence: `results/compiler-2026-10-16/penpot-phase42/`. Phase41's heap
snapshots found the discarded reader names; its live allocation totals are
not RSS measurements. The CLI already drops its host after analysis.

Previous validated cold rerun (phase33): main none **121.15 s / 1.17 GB RSS**, main
advanced **125.18 s / 1.32 GB**. Fresh shadow dev is **30.80 s / 2.21 GB**, release
**57.25 s / 4.10 GB**. The full suite passes **1,399 tests, zero failures,
106 ignored**. Strict Clippy, formatting and all three Node workflows pass.

Phase33 imports a macro call once and shares argument objects with
their elements in `&form`, matching upstream macro application. It
retains the decoded descriptor/value graph only across environment
import, then restores per-argument metadata and provenance in order.
Each argument still receives canonical snapshot validation. Whole-call
input conversion bounds already cover its argument subtrees.

Preparation or input release discards the graph. Snapshot failures
release newly assigned argument IDs and preserve the original error.
The bridge ABI is version 5; external evaluator adapters retain their
existing independent-import defaults.

Five new regressions cover shared identity and mutations with a reused
macro host; exact provenance for ratios, large integers, characters
and interned keywords after an intervening environment import; pending
graph cleanup on environment failure; partial snapshot failure; and
single construction of literal inputs. The provenance regression also
demonstrates why simply reusing existing values is insufficient.

Render is **15.50 s / 0.52 GB**. All three application bodies match phase32
outside the embedded runtime; syntax checks pass and the port is unchanged.
Evidence is in `results/compiler-2026-10-16/penpot-phase33/`.

Advanced remains **2.19 times slower** than shadow release at **32% of its peak RSS**; none is **3.93 times slower** than shadow dev at **53% of its RSS**. The requested speed goal is still unmet. Penpot browser execution
remains untested; scope differences and JVM RSS variability still apply.

Before phase42, the following diagnostics left production at phase33. The phase35 full-main
profile measures **54.67 s** in macro calls, **26.60 s** in macro transport,
and **19.60 s** in deferred analysis/emission after subtracting nested
instrumented work. Rumext `defc`/`html`, Instaparse and the two CSS macros
account for about 77% of the macro-call total. Uninstrumented callback work
remains included in its enclosing stage. Its complete output matches phase33.

Phase36's native sampling also reproduces the complete output. QuickJS's
interpreter accounts for about 40% of observed leaf samples; native runtime
operations and allocation are prominent too. Canonical comparison has only
six leaf samples and is not a large target in this profile. Recursive stack
counts are not additive time measurements; the full profile is archived.

Phase34's direct-method/mask prototype improves one macro replay but breaks
private-brand checks on forged objects, so it is rejected. Other store and
method variants have only small replay gains and are not retained. Phase37's
primitive-brand shortcut shows no measurable improvement and is rejected.
Evidence is in `results/compiler-2026-10-16/penpot-phase34/` through
`penpot-phase37/`; these diagnostics do not replace the clean table above.

Phase38 also rejects a QuickJS `Function.call` ownership optimization: repeated
macro replays show no useful gain. A deliberately invalid frame-removal variant
fails the stack-frame semantic probe and saves only about 1–2%. Both diagnostics
reproduce the captured result; neither changes production. Evidence is in
`results/compiler-2026-10-16/penpot-phase38/`.

Phase39 time-gated interrupt sampling again points to protocol dispatch and
declaration demand. Disabling demand after the replay's first call saves about
15%, but is not valid for general compilation. Native membership memoization
shows no useful gain. These diagnostic variants are not retained; the sampling
limitations and exact-result checks are recorded in `penpot-phase39/`.

Phase40 rejects a native Clang PGO build trained on the captured grammar:
its matching replays are about 13% slower than the ordinary build. No compiler
flags or profile data are retained. See `penpot-phase40/` for reproduction.

The previous phase32 comparison was main none **122.11 s / 1.28 GB**,
advanced **126.80 s / 1.39 GB**, shadow dev **30.73 s / 2.42 GB**, and
shadow release **55.79 s / 4.16 GB**. Its full suite passed 1,394 tests.
That phase fixed incorrect general-equality dispatch in record-field
lookup, contains and assoc, matching upstream keyword comparisons.
The captured Instaparse macro replay improved from 11.0 seconds to
9.56/9.42 seconds, but full main timing was essentially unchanged.

Phase30 counted 1,307,500 compiler-global reads in main; a ready-root
microbenchmark suggests only about 0.31 seconds, so another root cache
was rejected. Phase31 replayed the real Instaparse grammar macro with
identical encoded results: QuickJS took 11.0 seconds and Node 1.4 seconds,
against 11.1 seconds in the original compiler invocation. This identifies
JavaScript execution as the dominant cost for that macro, not a predicted
whole-build speedup or a decision to change engines. Both diagnostics
have separate evidence directories.

The previous phase29 comparison was main none **122.07 s / 1.27 GB**,
advanced **127.33 s / 1.30 GB**, shadow dev **30.20 s / 2.43 GB**, and
shadow release **55.20 s / 4.57 GB**. Its full suite passed 1,393 tests.
That phase removed string-table copying during runtime-parameter name
selection; all three complete outputs matched phase27 byte for byte.

Phase28's separate profile measured 5.165 seconds in string-table conversion
within 8.107 seconds of unit emission. Fragment assembly was only 0.388
seconds. Macro calls, including reader setup and reentry, were 62.998 seconds;
this is an inclusive measurement, not an exclusive CPU percentage. Its
outputs match phase27. Evidence is in `results/compiler-2026-10-16/penpot-phase28/`.

The previous phase27 comparison was main none **124.17 s / 1.29 GB**, advanced
**130.43 s / 1.40 GB**, shadow dev **30.18 s / 2.23 GB**, and shadow release
**54.72 s / 4.00 GB**. Its full suite passed 1,392 tests.

Phase27 avoids transporting `&env` to compiler-owned bootstrap macros whose
resolved bodies never read it. Source macros and reader wrappers keep the
complete view. Tight conversion bounds and compiler literals use the full
transport, preserving its failures. Three regressions cover the proof,
resource-limit recovery and literal metadata. Main none is 15.9% faster
than phase25; render is **15.70 s / 0.56 GB**. All three complete outputs
are byte-identical to phase25, Node syntax checks pass and the port remains
unchanged. Evidence is in `results/compiler-2026-10-16/penpot-phase27/`.

In the phase27 comparison, advanced was **2.38 times slower** than shadow release at **35% of its peak RSS**; none is **4.11 times slower** than shadow dev at **58% of its RSS**. The requested speed goal is still unmet. Penpot browser execution
remains untested; scope differences and JVM RSS variability still apply.

The previous phase25 comparison was main none **147.56 s / 1.31 GB**, advanced
**151.64 s / 1.43 GB**, shadow dev **33.53 s / 2.22 GB**, and shadow release
**56.60 s / 4.08 GB**. Its full suite passed 1,389 tests.

Phase25 caches fixed protocol metadata, indexes initialized extensions by
declaration order, and calls frontier entry/exit through an evaluator service
without reparsing JavaScript. Receiver properties and protocol methods stay
live. Regressions cover Proxy reentry during replay, inherited array properties,
original exceptions, cancellation and nested compiler callbacks. Render is
**19.91 s / 0.55 GB**. All three application bodies match phase6b outside
the runtime, Node syntax checks pass, and the port is unchanged. Penpot browser
execution remains untested. Evidence and the preliminary experiments are in
`results/compiler-2026-10-16/penpot-phase25/`.

In the phase25 comparison, advanced was **2.68 times slower** than shadow release at **35% of its peak RSS**; none is **4.40 times slower** than shadow dev at **59% of its RSS**. The requested speed goal is still unmet. Scope differences and JVM
RSS variability still apply.

The previous phase22 comparison was main none **159.03 s / 1.36 GB**, advanced
**162.93 s / 1.42 GB**, shadow dev **30.65 s / 2.30 GB**, and shadow release
**55.70 s / 3.98 GB**. Its full suite passed 1,384 tests.

Phase22 builds lists with their known stored counts, avoiding public protocol
dispatch on every newly created tail. It also fixes an ICounted override
corrupting a three-item list's stored count to 1000; explicit public count calls
still honor the override. The regression matches upstream ClojureScript 1.12.134
and passes at every optimization level and in macros. Main improves about 3%
from phase20; advanced peak RSS is about 7% lower in this observation. The
manifest's list-building effect targets are updated, all three Node workflows
and strict Clippy pass, and the port remains unchanged. Render is
**21.09 s / 0.54 GB**. Diagnostic motivation and poll-frequency limitations are
recorded in `results/compiler-2026-10-16/penpot-phase21/`.

Phase20 shares unchanged binding rows between adjacent lexical views and reuses
up to 128 immutable descendants in the environment bridge. Main none improves
9.5% from phase15, advanced 10.2%; peak RSS is essentially unchanged. References
resolve against the previous completed import, and cache publication requires
canonical snapshots. A new regression caught discarded map values losing their
numeric spelling on reuse; this is fixed without additional snapshot traversal.
Eviction order, shadow chains, conversion limits, reentry and failed imports
remain covered. All three Node workflows and strict Clippy pass.

All three generated application bodies match phase6b outside the embedded
runtime and pass Node syntax checking. Penpot application execution is still
untested. The clean five-job run and validation are retained in
`results/compiler-2026-10-16/penpot-phase22/`.
Advanced remains 2.93 times slower than shadow release at 36% of its measured
peak RSS; none is 5.19 times slower than shadow dev at 59% of its RSS. These
single-run RSS values include JVM variability and the documented scope differences.

Phase14 carries an arity index across namespace analysis instead of rescanning
the growing expression arena for each namespace. The index includes finalized
new expressions, unions redefinitions and overloads, and retains global identity
across namespaces. A regression passes with both the old and new implementations.
Standalone and deferred analysis retain their existing diagnostic scope.
The earlier five-job comparison is retained in
`results/compiler-2026-10-16/penpot-phase14/`.

Phase15 releases each parsed namespace tree after analysis has produced its
owned forms, retaining source text for diagnostics. All three complete outputs
are byte-identical to phase14 and pass Node syntax checking. Render is
**24.24 s / 0.53 GB**; main none's peak RSS is about 50 MB below phase14, while
advanced's peak and all elapsed times are essentially unchanged. The full suite,
strict Clippy and formatting pass. Evidence is in
`results/compiler-2026-10-16/penpot-phase15/`.

The diagnostic import breakdown (phase19, using the isolated phase13
profile source) narrows the remaining transport cost. For main, constructing
locals takes **17.06 s** and snapshotting them **6.61 s**: **23.67 s of 29.94 s**
inside the environment import service. Macro inventory construction and its
snapshot total only **0.56 s**; namespace data totals **0.41 s**, and reader-view
data **1.42 s**. The larger whole-view locals cache experiment did little because
most misses are distinct lexical views. This identified the phase20 target:
changing one binding reconstructed all of the view's binding records. The
phase20 prototype reduced locals construction to **7.18 s** and the entire
environment JS service to **18.96 s**. About 81% of binding rows on misses reuse
the previous prefix. These prototype measurements precede the discarded-child
correctness fix; the clean phase20 results above include that fix.
Raw render/main logs and output-equivalence checks are in
`results/compiler-2026-10-16/penpot-phase19/`. These are diagnostic measurements,
not additional clean benchmarks.

The original main problem was the amount of work and history retained per macro expansion
and per definition. The bridge repeatedly transports complete environments,
including all loaded macro metadata, and retains their provenance and canonical
copies for the entire compilation. Deferred analysis adds another multiplier by
copying whole namespace tables. Arity diagnostics repeatedly scan the expression
arena. These costs grow independently of the emitted JavaScript's size.

Investigated baseline: `4d2a0d69004dd1c993aab3a65e3080079858e4bc`.
The diagnostic investigation below used an isolated source archive in
`/private/tmp/light-cljs-investigation-20261016`. The subsequent production
changes and fresh measurements are recorded under **Implementation and rerun**
at the end; line references in the investigation describe the baseline.

## Complete `app.render` profile

This is the existing Penpot port, with all five source roots, raised macro
limits, `--esm --optimize none`, and a fresh release process. It analyzes
268 namespaces, 6,695 globals, 6,541 deferred units, and 476,709 expressions.
The diagnostic control completed in **254.42 seconds / 4,706 MiB peak RSS**.
The earlier uninstrumented benchmark reported 214.98 seconds / 4,686 MiB;
do not treat the instrumented control as a new production performance result.

| Analysis work | Exclusive seconds |
| --- | ---: |
| Convert environment descriptors into JavaScript values | 117.21 |
| Export the just-imported environment back into descriptors | 47.21 |
| Decode and retain the environment snapshot in Rust | 14.83 |
| Encode the environment in Rust | 13.91 |
| Execute macro functions, including their reader-context wrapper | 15.75 |
| Collect expression warnings | 12.32 |
| Everything else in analysis | 28.54 |
| Total profiled analysis | 249.77 |

Environment import, including its small surrounding overhead, consumes
**193.28 seconds, 77% of profiled analysis**. Only 10,372 JavaScript macro
expansions are involved. Merely constructing the environment in Rust takes
about one second; transporting it is the expensive part.

The supplied `app.main` sample agrees with this diagnosis: of 15,987 samples,
12,680 (79.31%) include `import_data`, and 2,423 (15.16%) include
`collect_expression_warnings`. These are inclusive counts with repeated
ancestors deduplicated. This is a roughly 16-second slice of an earlier build,
not a whole-build percentage or a profile of the final 2,057-second run.

## 1. The macro ABI repeatedly round-trips large immutable data

Relevant code:

- `src/macro_host.rs`, `compiler_environment` (line 1218): rebuilds locals,
  namespace imports/requires, and `:light-cljs/macros`. The latter sorts and
  includes **every loaded source macro's metadata**, not just the called macro.
- `src/macro_host.rs`, `expand_javascript` (line 161): imports `&form`, imports
  `&env`, then imports each explicit argument separately. Argument subtrees
  already transported in `&form` are transported again.
- `src/evaluator.rs`, `ActiveEvaluator::import_data` (line 1109): encodes Rust
  data, imports it into JavaScript, immediately exports it again, and decodes a
  canonical Rust snapshot. This happens on every import.
- `src/form_bridge.rs`, `encode_node` / `decode_node`: recursively allocate
  descriptors and compiler values. Existing identities do not skip encoding.
- `src/js_runtime.js`, `compilerFormBridge` (line 3667): creates actual runtime
  collections, names, metadata and traversal maps from those descriptors.

Snapshotting supports real semantics: unchanged syntax recovers original source
locations and reader distinctions, while mutated `#js` and Date values must not
reuse stale compiler data. Applying that mechanism to every environment and
every immutable subtree is extremely expensive.

The cost is proportional to expansion count times transported environment size.
Environment size increases with locals, imports and the loaded macro inventory.
A trivial `when` therefore becomes more expensive as unrelated compiler state
grows. Rebuilding the full metadata map is especially wasteful when a macro
never inspects it.

A controlled reproduction makes this visible. Each file contains the same
1,000 functions using `when`, preceded by different numbers of unused identity
macros. All three runs execute 1,024 JavaScript macro expansions, including
bootstrap work:

| Unused macro declarations | Wall seconds | Environment work, seconds | Peak RSS, MiB |
| --- | ---: | ---: | ---: |
| 0 | 1.01 | 0.50 | 99.1 |
| 50 | 6.80 | 6.10 | 225.7 |
| 100 | 12.94 | 12.08 | 354.0 |

Environment work includes building and importing it. Macro execution itself
takes only 0.036–0.048 seconds. Adding 100 unused macros makes this workload
nearly 13 times slower, overwhelmingly by increasing transport work.

`withMacroReaderContext` also copies JavaScript Maps/Sets on each call. It should
be improved, but the complete macro-call bucket, which includes that wrapper,
is only 15.75 seconds here. The bridge deserves priority over that wrapper.

## 2. Releasing JavaScript roots does not release bridge history

`BridgeState` owns strong references in `originals`, `identities`, `canonical`
and `forms` (`src/form_bridge.rs`, line 26). Encoding appends original data;
snapshot decoding stores canonical copies. There is no pruning during the
session. `Evaluator::release` only deletes the JavaScript value handle
(`src/evaluator.rs`, line 1097).

For `app.render`, the bridge ended with:

- 3,048,882 original entries, including 2,917,040 newly retained by environment
  imports;
- 2,388,812 canonical entries and 131,000 retained form entries;
- **2,527,298,561 bytes freed by dropping the bridge's Rust tables**;
- 119,662,687 bytes reported by QuickJS at evaluator teardown.

Live requested Rust allocations at the end of analysis were 4,084,066,555 bytes.
The bridge's measured release alone is about 62% of that amount. Requested
allocation bytes and RSS are different metrics; the QuickJS number is a teardown
observation, not a measurement of its peak heap.

This is compilation-lifetime retention, not evidence that memory survives
process teardown. Increasing JavaScript GC frequency cannot free those strong
Rust references. The `--macro-memory-limit` allocator bounds QuickJS allocation;
it is not a limit on the whole Rust compiler process.

## 3. Deferred snapshots still copy whole namespace tables

`Shared<T>` is an `Rc<T>` with `Rc::make_mut` on mutation
(`src/shared.rs`, line 35). For `Shared<HashMap<String, RuntimeReaderNamespace>>`,
that means cloning the whole map and its owned alias/refer/name vectors.

Each deferred unit starts from a cloned `resolution_program`, then inserts its
reader context (`src/analyzer.rs`, lines 3251–3277). It later mutates the same
table's macro list. `CompilationSession` retains both the definition snapshot
and the analyzed `Program` (`src/compilation_session.rs`, lines 12–54).

Consequently, sharing the original table does not make subsequent per-unit
mutation cheap. With D deferred definitions and N namespaces, this path can
retain O(D × N) namespace records, including their strings and vectors.
Other shared tables and copy-on-write arena chunks also deserve auditing.

The catalog audit in the combined `app.render` ablation below found
**6,798 distinct reader-context maps,
869,893 namespace records and 3,691,021 alias entries** retained across the
6,541 units' snapshots and analyzed programs. Reader-table cloning allocated
about 510 MB cumulatively; that is a copy-allocation counter, not peak RSS.

A separate fixture contains 40 plain identity functions per namespace, with ten
aliases of the preceding namespace. It invokes no JavaScript macros:

| Namespaces | Definitions | Retained namespace records | Reader-table copy allocations, MB | Peak RSS, MiB |
| --- | ---: | ---: | ---: | ---: |
| 25 | 1,000 | 13,300 | 9.14 | 63.8 |
| 50 | 2,000 | 52,225 | 38.51 | 168.5 |
| 100 | 4,000 | 206,950 | 158.24 | 432.5 |

Doubling the workload approximately quadruples this copying. The original
single-namespace scaling fixture cannot expose this namespace multiplier.

## 4. Arity diagnostics perform repeated whole-program searches

`collect_expression_warnings` iterates each new call expression. For a global
callee, it searches the expression arena for definitions of that global
(`src/analyzer.rs`, line 4024). At the outer namespace pass, the search starts
at expression zero. Calls inside function bodies participate too.

The `app.render` run examined **3,358,001,433 expression candidates** in these
arity searches. This is O(global calls × expression count), rather than a
lookup by `GlobalId`. It runs even when this CLI invocation does not print
warnings. The deferred-unit warning pass also means some call analysis occurs
again when the assembled namespace is checked.

An indexed, declaration-aware arity table would preserve warning semantics
without these scans. It must retain the current treatment of redefinitions,
constructors, variadic methods, metadata wrappers, and deferred visibility.

A fixture with N functions, each calling the same global function, reproduces
the quadratic scan independently of JavaScript macro expansion:

| Functions | Expressions examined for arities | Warning-pass seconds |
| --- | ---: | ---: |
| 1,000 | 16,987,995 | 0.043 |
| 2,000 | 67,975,995 | 0.173 |
| 4,000 | 271,951,995 | 0.707 |
| 8,000 | 1,087,903,995 | 2.844 |

## Diagnostic ablation

The isolated build has switches to import environment values without registering
provenance identities or taking their canonical snapshots, and to skip the
expression-warning pass. The actual environment values still reach the macro.
This does not cache environments or avoid their JavaScript construction.

| `app.render` experiment | Wall seconds | Peak RSS, MiB | Bridge bytes freed at teardown |
| --- | ---: | ---: | ---: |
| Instrumented control | 254.42 | 4,706.3 | 2,527,298,561 |
| Environment provenance/snapshot disabled; warning pass disabled | 159.90 | 2,102.7 | 131,361,521 |

The emitted 12,616,500-byte JavaScript files have the same SHA-256 hash:
`f7cbd4bdce25d5f7a5bfb1f5a370975bac78d6aa6a2f5605d5a4ddd5660b6b64`.
This run is 37% faster and uses 55% less peak RSS. Environment construction in
JavaScript still takes 106.95 seconds, so this leaves the largest remaining
CPU cost intact. It is evidence for the diagnosis, not a production-ready fix
or a claim that these changes close the shadow-cljs gap.

The original 4,000-function single-namespace fixture was also run with only
environment provenance/snapshot disabled: 16.91 → 13.61 seconds and
737.7 → 414.0 MiB RSS, with identical output. The untouched release binary
took 16.93 seconds / 727.8 MiB on that fixture.

## Other avoidable work

Deferred analysis rebuilds the global-name lookup map from the snapshot's full
global arena (`src/analyzer.rs`, line 3252). Macro expansion also constructs a
fresh `LineIndex` by scanning the entire source file, even though `Source`
already has a cached line index (`src/analyzer.rs`, lines 9069 and 9166;
`src/compiler_data.rs`, line 137). These are concrete secondary targets, but
the measurements above do not assign them individual percentages.

## Fix order

1. Give environment transport a bounded lifetime and avoid canonical snapshots
   for data that does not need syntax provenance. Preserve escaped source forms,
   nested/reentrant expansion and mutable-literal behavior; clearing all bridge
   tables after every macro is not a safe general fix.
2. Reuse immutable JavaScript environment components by namespace/macro revision.
   Transport local changes, rather than all aliases and all macro metadata on
   every expansion. Avoid importing explicit arguments twice. Keep reflection
   and reader visibility correct.
3. Replace whole namespace-table copies with structural sharing and per-unit
   changes. Release completed units' obsolete analysis context when its consumers
   no longer need it. Audit the remaining `Shared` mutations and arena copies.
4. Index global arities, then reuse global lookup maps and source line indexes.
5. Measure again before changing engines or parallelizing analysis. A JVM JIT
   and concurrency help shadow-cljs, but upstream's analyzer also directly
   passes its existing `form` and `env` to the macro function
   (`research/clojurescript/src/main/clojure/cljs/analyzer.cljc`, line 4309).
   This implementation adds repeated conversion and retained copies at that
   boundary. More workers would also multiply its current memory demand.

The original `none`/`advanced` timings establish that optimization is small
relative to this frontend cost, not that optimization literally costs zero.
The scope and port differences prevent a precise apples-to-apples speed ratio,
but they cannot explain the measured billions of scans and repeated bridge
round trips. The surfaced compatibility gaps are separate correctness work.

## Evidence and limitations

Raw logs, the isolated diagnostic patch, fixture generator and output hashes
are in [the evidence directory](results/compiler-2026-10-16/penpot-investigation/).
These are sequential, single-observation diagnostic runs, not a statistical
benchmark suite. Instrumentation counts requested allocation bytes and exclusive
phase time, and adds overhead. The initial investigation did not rerun the full
34-minute `app.main` build.
All ten generated synthetic modules were executed in Node 24.18.0 and returned
the expected results. The diagnostic patch passes `git apply --check` against
the investigated source. The initial investigation did not rerun the production suite.

The diagnostic switches deliberately remove work to establish causality. They
are not production fixes. Penpot output has not been bundled or executed, and
matching emitted bytes cannot establish that the original port is correct.

## Implementation and rerun

The working-tree implementation replaces the session-wide Rust provenance
registry with opaque engine-owned handles. Object provenance lives in a weak
map and is collected with the imported object. Macros that save objects retain
their source forms, canonical payloads and lexical distinctions. Scalars have
no observable object lifetime, so their latest provenance is retained by value,
as before, rather than by expansion. Leaf provenance holds a single form node,
not the entire input syntax arena. Source text remains available for diagnostics.
Reusing a collection restores provenance for all its original children,
including number spellings that collapse to the same JavaScript Number.

Environment transport caches up to 64 immutable top-level components per
evaluator. Macro metadata is built once per macro inventory revision and shared
by equivalent definition frames; namespace metadata is reused while its inputs
match. Cache hits send a reference and charge the original logical node/depth
budget. Fresh components still take canonical snapshots. Components containing
host literals are copied and checked on every import. This preserves mutation
isolation, macro reflection, reader metadata and escaped environments. Failed
imports invalidate the native cache so interactive recovery cannot refer to an
engine slot that was never installed.
Reentrant imports during cache population use the ordinary bridge: constructing
maps can invoke user hashing/equality code, so nested analysis must not see
pending cache slots before their JavaScript values exist.

Reader-context tables now share a layered map and individual namespace records.
A namespace is registered before definition snapshots are taken, and unchanged
contexts avoid writes. JavaScript reader contexts use overlays instead of
copying their parent Maps and Sets during macro expansion; nested calls restore
the prior view in `finally`. Arity warnings index the same declaration range
once per pass, preserving constructor signatures, metadata wrappers, overloads,
variadic methods, redefinition unions and deferred-unit visibility.

The shared runtime grows by 1,344 bytes for the bridge and reader machinery.
The `none`/`prepare` size ceilings increase by 2,000 bytes to cover it; advanced
ceilings remain unchanged because ordinary application slices discard these
compiler-only units.

The benchmark driver is [penpot.bb](penpot.bb). It runs one build process at a
time, uses all five existing port roots and the original macro limits, and
clears only shadow's generated compiler cache before each shadow invocation.
It does not regenerate or modify the port. Large outputs stay outside Git.

Validation on the final implementation:

- `cargo test --release --no-fail-fast`: **1,363 passed, 0 failed, 106 ignored**.
  This is the same regular suite used for the original 1,355-pass baseline;
  optional ignored reference/oracle suites were not enabled.
- `cargo fmt --check` and `cargo clippy --locked --all-targets -- -D warnings`: passed.
- The three Node checks in the conformance workflow (`js_runtime`,
  `js_core_helpers`, `execution`): passed.
- New tests cover native provenance reclamation, scalar retention by value,
  saved subforms and scalars across GC, immutable environment reuse, mutable
  literal isolation, logical conversion limits, failed-import recovery,
  reentry during environment construction, and indexed arity diagnostics.

Measured host: macOS `VirtualMac2,1`, 24 logical CPUs, 32 GiB RAM;
Rust 1.99.0 and Node 24.18.0. The 996 files under the five port roots had a
combined path/content SHA-256 of
`b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`
before the rerun. Each compiler invocation is a fresh process; shadow's cache
is cleared, while OS filesystem caches and installed dependencies are retained.
The same input hash was verified after all builds.

### Penpot results

RSS below is in **GiB** (bytes divided by 2³⁰), calculated from the raw
`/usr/bin/time -l` logs rather than the original rounded GB labels.

| Build | Original wall, s | Fixed wall, s | Speedup | Original RSS, GiB | Fixed RSS, GiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| `app.main --optimize none` | 2,057.49 | **525.39** | **3.92×** | 18.59 | **5.27** |
| `app.main --optimize advanced --pseudo-names --elide-asserts` | 2,043.00 | **568.97** | **3.59×** | 18.57 | **5.32** |
| `app.render --optimize none` | 214.98 | **59.63** | **3.61×** | 4.58 | **1.38** |

Peak RSS falls by 71.6%, 71.4% and 69.9%, respectively. The initial new
`main-none` run, which was sampled, took 570.40 seconds and 5.23 GiB. Its clean
repeat is the 525.39-second row above. This variation is larger than the sample
interval itself; do not attribute it all to profiler overhead or treat these
single observations as precise optimizer phase measurements. Both main builds
still spend almost all their wall time on one core.

Fresh shadow measurements, with its compiler cache removed for each run:

| Build | Wall, s | User CPU, s | System CPU, s | RSS, GiB | Build summary |
| --- | ---: | ---: | ---: | ---: | --- |
| `release main` | **60.88** | 147.03 | 456.88 | **3.75** | 1,176 files; 916 compiled; 0 warnings |
| `compile main` | **35.77** | 92.13 | 494.86 | **2.04** | 1,209 files; 1,189 compiled; 0 warnings |

The advanced-to-release wall-time gap is now **9.35×**, versus 36.5× in the
original measurements. The dev comparison is still 14.7×. Scope remains
different: shadow includes render, rasterizer and code-highlight; light's
main includes the app and its five lazy modules, with render measured separately.
These fixes substantially reduce the gap but do not make the compiler fast
relative to shadow.

### Scaling checks and output verification

The same untouched baseline binary and the final release binary compiled each
synthetic fixture sequentially, outside the Penpot timed runs:

| Fixture | Baseline wall, s | Fixed wall, s | Baseline RSS, MiB | Fixed RSS, MiB |
| --- | ---: | ---: | ---: | ---: |
| 8,000 functions calling a global | 7.22 | 4.22 | 521.1 | 525.0 |
| 100 namespaces × 40 identity functions | 1.95 | 1.72 | 437.3 | 232.6 |
| 1,000 macro-using functions; 0 unused macros | 1.02 | 0.93 | 100.6 | 84.0 |
| Same; 50 unused macros | 6.80 | 1.21 | 225.9 | 107.0 |
| Same; 100 unused macros | 12.95 | 1.48 | 343.2 | 97.3 |
| Original 4,000-function `when`/`cond` fixture | 16.66 | 14.86 | 722.5 | 343.8 |

The unused-macro multiplier has largely disappeared, and namespace snapshots
use much less memory. The call fixture still grows roughly quadratically:
1,000/2,000/4,000/8,000 functions take 0.11/0.34/1.12/4.22 seconds after the fix.
The original `when`/`cond` fixture saves memory much more than time. These
results support the remaining-work list below; removing arity scans and reader
table copies does not remove every source of repeated analysis work.
All **22 baseline/fixed synthetic modules executed in Node and returned the
expected values**.

All three Penpot outputs pass `node --check`. Render's application code matches
the original byte-for-byte after excluding the embedded runtime. The current
port's generated font catalog already contains **1,910 different font UUIDs**
from the old main-build artifacts. The new emitted IDs/UUIDs match that source
exactly. Mapping each current UUID to the old UUID for the same font ID makes
both main application bodies byte-identical to their old outputs; there are no
other application-code differences. The advanced embedded runtime is also
byte-identical. The none/render runtime grows by the documented 1,344 bytes.
The sampled and clean new `main-none` outputs are byte-identical to each other.

Penpot was not bundled or run. The checks establish syntax and preservation of
the emitted application code, accounting for the existing font-input difference;
they do not establish correctness of the original port. Raw timing logs,
commands, input/output hashes, tests and verification scripts are retained in
[the fixes evidence directory](results/compiler-2026-10-16/penpot-fixes/).

### Remaining work after these fixes

A five-second, 10 ms sampling interval late in the first new `app.main` run
collected 449 main-thread samples. Attributing each sample to its innermost
compiler phase gives 39.2% in macro calls, 19.8% in environment import, 11.1%
in JavaScript unit evaluation, 10.2% in ordinary form import, 4.5% in expression
warnings, and 4.2% in form export. Nested analysis is attributed separately.
The earlier sample put 79.3% in ordinary `import_data`, which then included
environment transport. The samples cover different short intervals; they show
the changed bottleneck, not whole-build phase percentages or an exact phase
speedup. The sampled run is recorded separately from the clean timing.

Analysis remains single-threaded. Ordinary form transport still imports
`&form` and explicit arguments separately and takes canonical snapshots.
Reader visibility and locals still travel with each expansion. Deferred units
still rebuild global-name lookup maps, and several other shared tables and
arena chunks can still copy when written. Source line indexes are still
reconstructed at expansion sites. The arity fix removes the scan for every
global call, but it still builds an index for each diagnostic pass. These are
remaining optimization opportunities; this change does not claim to remove
every source of nonlinear scaling or reach shadow-cljs performance.

## Second optimization pass

Starting from `415620c` (the first fixes), this pass addresses additional copying
and repeated work that the original hotspots had obscured:

- Deferred frames now share a persistent global-name trie. They no longer
  reconstruct a `HashMap` from every global in the definition snapshot.
- Deferred analysis reuses its source object and cached line index. An empty
  fragment extension no longer invokes `Rc::make_mut` on shared metadata tables.
- Completed units consume and release their original form, definition snapshot,
  and analysis host; their analyzed program remains available for linking and
  later macro demand.
- Frozen AST chunks are trees of leaves containing at most 64 entries. Merging
  chunks shares roots instead of cloning historical entries; a write copies a
  path and one bounded leaf. A regression test holds 300 snapshots of 21,000
  entries and verifies that freezing/merging clones no entries.
- Arena iterators jump directly for `nth`/`nth_back`, so `skip(prefix)` does not
  walk every old entry. This matters in deferred linking and fragment append.
- Compile-time units omit unused local declarations for globals held in the
  demand registry. Successful protocol-demand lookups are cached within an
  unchanged registration/frontier view; failures and reentrant changes are not
  cached, and entering the bootstrap frontier skips user-extension demand.
- Bridge strings and symbol/keyword components use QuickJS's native UTF-16
  string API instead of per-code-unit arrays. Descriptor ABI 3 preserves empty
  strings, NUL, all 65,536 code units, lone surrogates, and a null namespace for
  unqualified names. Mutable payload snapshots and provenance remain intact.

The intermediate tree implementation reduced memory but regressed CPU time:
8,000 plain definitions took 4.48 seconds, versus 0.87 seconds immediately before
that change. A separate sampling run attributed 1,797 of 2,356 main-thread
samples to iteration in `drain_deferred`, with another 288 in fragment append.
Those paths were walking old globals just to skip them. Direct iterator jumps
remove that work; the final probe takes 0.61 seconds. The intermediate regression
is not the production result.

| Probe | First pass, seconds | Second pass, seconds | First pass RSS, bytes | Second pass RSS, bytes |
| --- | ---: | ---: | ---: | ---: |
| 8,000 functions calling a global | 4.22 | 0.61 | 550,469,632 | 378,552,320 |
| 100 namespaces, 4,000 definitions | 1.72 | 0.75 | 243,941,376 | 77,463,552 |
| 4,000 functions with `when`/`cond` | 14.86 | 11.68 | 360,497,152 | 271,302,656 |

These are individual cold runs, not statistical estimates. In particular,
removing table copying helps plain definitions much more than macro-heavy code.

The full suite passes **1,372 tests, 0 failed, 106 ignored**. Strict Clippy,
formatting, and the three Node runtime/helper/harness workflows pass. All 996
port inputs retain the earlier path/content hash. Evidence and reproduction
scripts are in [the second-pass directory](results/compiler-2026-10-16/penpot-phase2/).

### Remaining costs measured over the whole render build

A separate instrumented render run takes 44.59 seconds and 823,869,440 bytes
peak RSS. Its exclusive analysis buckets are:

| Work | Seconds |
| --- | ---: |
| Environment import | 11.40 |
| Macro invocation, including unscoped demand work | 10.09 |
| Import `&form` | 3.20 |
| Import explicit arguments again | 2.68 |
| Export expanded forms | 2.70 |
| Emit namespace registrations | 3.99 |
| Other unit emission | 1.14 |
| Evaluate units, including frontier operations | 2.23 |
| Warnings | 1.01 |
| Build environments and reader contexts | 0.37 |
| Other analysis | 5.22 |

Nested named scopes are subtracted from their caller. Uninstrumented nested
work, including some demanded helper analysis, remains in the caller's bucket;
these are not pure JavaScript CPU totals.

There are still 10,372 JavaScript macro expansions and 269 emitted compile-time
units. Building the environment in Rust is cheap compared with importing it.
The bridge's identity cache covers top-level components, but locals and reader
contexts are freshly constructed each time. Most bootstrap macros never read
`&env`, yet receive its full transport. Avoiding that work must preserve reader
registration acknowledgments, resource limits, source macro wrappers, and the
few bootstrap macros that actually inspect the hidden arguments. Namespace
registration also still emits all visible namespace catalogs per demanded unit.
These measured costs remain targets; this pass does not meet the goal of
outperforming shadow-cljs in both time and memory.

Other retention candidates still need measurement: macro-host frames retain
`Layered` maps whose own layers are copied on clone, and demanded units cache
emitted strings containing repeated namespace catalogs. The persistent global
name index does not replace those other maps. Neither candidate has been
assigned a share of the remaining main-build RSS by this pass.

### Clean second-pass Penpot results

| Build | Wall seconds | Peak RSS, GiB | Output bytes |
| --- | ---: | ---: | ---: |
| light-cljs render, none | 45.04 | 0.759 | 12,617,418 |
| light-cljs main, none | 481.76 | 2.977 | 33,583,432 |
| light-cljs main, advanced | 481.44 | 3.027 | 25,663,355 |
| shadow-cljs release main | 60.38 | 4.323 | — |
| shadow-cljs compile main | 31.80 | 2.122 | — |

The original-to-current main improvement is about **4.3× in time and 84% in
peak RSS**. Against the first fixes, main-none time improves only 8.3%, while
peak RSS falls 43.5%. The memory changes therefore do not close the CPU gap.
Current advanced is 7.97× slower than fresh shadow release; none is 15.15×
slower than shadow development and uses more peak memory. The earlier scope
caveat still applies: shadow emits more application modules in this one job.

Both shadow builds report zero warnings. Release compiled 916 of 1,176 files;
development compiled 1,189 of 1,209 files. The driver removed generated shadow
compiler caches before each process. None of these five timing runs was sampled
or overlapped with another compiler/test build.

All three application bodies are byte-identical to the first-pass outputs when
the embedded runtime is excluded. The render and none runtimes shrink by 426
bytes; advanced is byte-identical in full. All three pass `node --check`, and
the three new synthetic modules execute their expected results in Node. The
input hash remains `b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`.
This verifies preservation of emitted application code, not Penpot runtime
correctness: the port still has not been bundled or run.

### Whole-main second-pass profile

A separate instrumented main run completed in 472.40 seconds at 3,193,061,376
bytes peak RSS, producing byte-identical output to the clean main-none run.
There were 48,216 macro expansions and 441 emitted compile-time units. The
exclusive buckets were 128.13 seconds in environment import, 156.56 seconds in
macro calls (including uninstrumented nested demand work), 67.47 seconds in
namespace registration emission, 22.06 seconds importing the whole call,
18.63 seconds importing explicit arguments, 16.15 seconds exporting forms,
12.25 seconds evaluating units/frontiers, 9.99 seconds in warnings, 6.40
seconds in other unit emission, 4.10 seconds building environments, and 28.33
seconds in other analysis. Raw evidence is `penpot-phase2/profile-main.log`.

Environment transport, repeated namespace registration, and macro execution
remain the leading costs at full application scale. Warning collection is
now a comparatively small part of this measured run.

## Third optimization pass

Compiler environments reuse one unchanged local-binding map and one unchanged
reader-context map across analysis frames. Their keys include the complete
ordered local descriptions (including shadows), or the reader declaration
generation/cursor/visibility, refers, excludes and own bindings. Reader
acknowledgment remains separate from reuse. Namespace emission groups globals
once instead of scanning the entire global table for every namespace. Failed
macro calls now release successfully imported arguments on ordinary exceptions.

The full suite ran 1,375 tests: all existing tests and the new failure cleanup
and reader-view tests passed. The new lexical-cache test initially called the
sequence inspection API on an empty map; after correcting that assertion, both
cache regression tests passed. Strict Clippy and formatting passed. Evidence
is in `results/compiler-2026-10-16/penpot-phase3/`.

Clean render takes **36.65 seconds and 821,428,224 bytes peak RSS**. Its output
is byte-identical to phase2f. The 4,000-function macro probe falls from 11.68
to 9.18 seconds; the two plain-definition probes are essentially unchanged.
A separate render profile attributes 7.38 seconds to environment import
(previously 11.40), and 0.62 seconds to namespace registration (previously
3.99).

The separate main profile takes **325.23 seconds and 2,911,387,648 bytes peak
RSS**, also producing byte-identical output. This run includes a two-second
sampling window and a terminal heap inspection/GC; it is a diagnostic run,
not a clean benchmark. Environment import falls from 128.13 to 48.28 seconds;
namespace registration falls from 67.47 to 3.18 seconds. Macro execution and
its uninstrumented nested work remain expensive: `rumext.v2/defc` takes
42.90 seconds, one Instaparse grammar expansion 31.22 seconds, and the two
style macros 30.73 seconds combined.

QuickJS reports 1,014,816,592 allocated bytes before explicit GC and
792,560,272 afterward. Function records account for 682,836,605 bytes, versus
31,980,997 bytes of bytecode. The next diagnostic separates retained function
source from variable/constant metadata. A late two-second CPU sample also
finds substantial native Map/WeakMap lookup and strict-equality work; the
vendored engine's bucket hash preserves pointer alignment and small-integer
encoding patterns, making it a further candidate for investigation.

## Engine hashing and completed initializer retention

QuickJS's native Map hash multiplied aligned pointer bits and encoded numbers
without mixing high bits into the low bucket bits. A final avalanche mixer fixes
this without changing key equivalence or insertion order. A 20,000-key probe with
ten read rounds falls from 152 to 15 ms for numeric keys and 78 to 14 ms for
object keys. This microbenchmark does **not** establish a Penpot speedup: clean
render remains 36.70 seconds versus 36.65 before the change.

The vendored engine's build script also failed to track its original C sources
and headers after copying them into Cargo's output directory. The first hash
trial therefore used the old engine. Explicit `rerun-if-changed` entries now
ensure rebuilds. The phase4b measurements and phase5 validation use the rebuilt
engine; the initial phase4 trial is not valid evidence for the hash change.

A detailed render heap profile finds 85.1 MB of retained function source out
of 85.8 MB of function records. Completed compiler roots and declaration bundles
kept their initializer closures after storing the resulting values or errors.
Each initializer retained its generated source, including namespace catalogs.
The Rust compilation session also cached emitted source which successful roots
never requested again. Completed/failed initializers now release their closures,
and the session retains an emission flag rather than the source string. Deferred
analyzed programs remain available for later compilation demands.

The full suite passes **1,376 tests, zero failures, 106 skipped**. Strict Clippy
passes. Clean render takes **34.25 seconds and 630,505,472 bytes peak RSS**;
its complete output is byte-identical to phase2f and passes `node --check`.
Evidence is in `results/compiler-2026-10-16/penpot-phase5/`.

The separate main profile takes **285.24 seconds and 1,622,540,288 bytes peak
RSS**. Its application body is byte-identical to phase2f; the embedded runtime
differs only by reader timing instrumentation. It includes diagnostic counters
and a terminal GC, so is not a clean benchmark. Retained function source falls
to **1,579,653 bytes** and function records to 2,352,621 bytes. QuickJS reports
102,269,672 allocated bytes before GC and 77,168,368 afterward. Overall malloc
usage remains much larger, at about 1.08 GB before GC: remaining memory is
predominantly outside the engine. The final program has 1,220,906 expressions
of 160 bytes each, before their separately allocated payloads and analysis views.

Reader setup alone takes **44.40 seconds over 20,126 wrapped macro calls**.
Environment import takes 39.70 seconds, whole-form import 16.55, argument import
14.28 and result export 12.63. This still leaves a substantial time gap against
shadow-cljs; the memory improvement does not establish the requested speed goal.

## Protocol lookup and fresh cold benchmarks

Generic collection operations repeatedly searched the fixed protocol-mask table
and constructed its property names. A precomputed map now supplies each protocol's
property and bit, retaining the last occurrence of duplicate ABI entries. Method
selection, declaration demand, native fallbacks and custom protocol calls are
unchanged. The first suite run caught a five-byte output-size ceiling overrun;
storing each entry as a compact pair fixed it without moving any ceiling. The
full rerun passes 1,376 tests, zero failures, 106 skipped; strict Clippy and the
three direct Node runtime/helper/execution workflows pass.

All five jobs were rerun sequentially with fresh processes and cold compiler
caches. The port still contains 996 files with combined path/content SHA-256
`b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`.
Phase6 evidence is in `results/compiler-2026-10-16/penpot-phase6/`.

| Build | Wall seconds | Peak RSS bytes | Output |
| --- | ---: | ---: | --- |
| light-cljs render | 32.48 | 601,751,552 | 12,617,448 bytes |
| light-cljs main none | 276.60 | 1,556,643,840 | 33,583,462 bytes |
| light-cljs main advanced | 283.96 | 1,672,003,584 | 25,663,385 bytes |
| shadow-cljs release main | 60.53 | 3,956,981,760 | 1,176 files, 916 compiled, zero warnings |
| shadow-cljs compile main | 33.25 | 2,182,217,728 | 1,209 files, 1,189 compiled, zero warnings |

Main none is 7.44 times faster than the original 2,057.49-second build. Advanced
still takes 4.69 times shadow release's wall time, although its peak RSS is 42%
of shadow release's. None takes 8.32 times shadow dev's wall time, at 71% of its
peak RSS. Scope differences remain: shadow also builds render, rasterizer and
code-highlight, while light main includes its five lazy modules and render is
measured separately.

All three application bodies are byte-identical to phase2f after removing the
embedded runtime; the protocol lookup optimization changes that runtime by
30 bytes. All three complete outputs pass `node --check`. The Penpot application
has still not been bundled and run, so this is output preservation and compiler
validation, not a claim of end-to-end application correctness.

## Further diagnostic and import cleanup

The checked-array warning path had another whole-program search for each array
access, to determine whether it was an assignment target. It now builds the same
target set once per warning pass, and only when checked-array expressions exist.

An imported value also remained rooted if later canonical validation failed
before the caller received its ID. Both ordinary and environment imports now
release that temporary root on failure, preserving any pending JavaScript
exception across cleanup. The new regression repeatedly supplies invalid UUID
and instant values through the ordinary, outer-environment and active-environment
paths, checks provenance collection, and verifies subsequent successful use.
The full suite passes **1,377 tests, zero failures, 106 skipped**, and strict
Clippy passes. These two changes follow the phase6 benchmark binary; their
storage profile and timing measurements are separate.

The phase7 isolated main profile completed in 282.36 seconds, at 1,629,192,192
bytes peak RSS. Instrumentation and terminal collection make this a diagnostic
run, not another clean timing result. Its application body matches phase6b byte
for byte after excluding the instrumented runtime. Evidence is in
`results/compiler-2026-10-16/penpot-phase7/`.

Environment transport spends 2.63 seconds encoding Rust descriptors, 35.07 in
JavaScript conversion and 2.26 validating snapshots in Rust. Reader preparation
spends 35.12 seconds rebuilding aliases, 5.21 reading the remaining view fields,
1.08 registering declarations and 0.04 constructing overlays. Thus alias work
and canonical collection construction remain significant even after namespace
snapshot sharing; Rust environment construction is no longer the main cost.

Deduplicating shared arena chunks by identity across the main program and every
retained unit finds 2,421,603 expression entries occupying **470,467,984 bytes**
of table allocation (including spare capacity and chunk headers). Other tables
account for about 75 MB. These counts exclude separately allocated expression
payloads. The final main program itself has 1,220,906 expressions. After terminal
GC, live bridge data is 213,231 Datum nodes (64 bytes each including their Rc
header), 21,351 form nodes and about 2.2 MB of form buffers. QuickJS's allocated
heap is 77.2 MB; macOS malloc reports 1.07 GB still used overall. Remaining memory
is primarily compiler program storage and its payloads, not retained bridge
forms or QuickJS function source.

## Compact expression storage

The largest expression variant previously made every expression occupy 160
bytes on the benchmark machine. The constructor IDs, body, masks and fields of
a type definition now live in a boxed `TypeLayout`, reducing `Expr` to **96
bytes**. Definition cloning still clones the layout, and all readers, remappers
and optimization passes retain the same fields and ordering.

The full suite remains at 1,377 passed, zero failures, 106 skipped, with strict
Clippy passing. The isolated clean render build takes 32.50 seconds and
571,506,688 bytes peak RSS, versus phase6b's 32.48 seconds and 601,751,552 bytes.
The complete JavaScript output is byte-identical. This is a measured memory
reduction, not a speed improvement; main measurements follow separately.

## Less temporary work during macro execution

Bridge decoding now keeps its iterative work stack in one flat array and uses
direct descriptor slots and a switch, avoiding a temporary pair and tag arrays
for every node. The descriptor-to-value map, postorder, metadata handling and
snapshot validation are unchanged. Compiler declaration demand caches method
protocol names, and selector caches use nested native-type/type-name keys rather
than concatenating a string for every lookup. Registration, frontier reentry,
exceptions and declaration effects retain their existing invalidation behavior.

This phase passes all 1,377 tests, strict Clippy and the three direct Node checks.
One cold render measured 31.53 seconds and 573,931,520 bytes RSS. This small time
change needs the full main measurements before drawing a broader conclusion.

Fixed type-brand inventories also used to allocate an array and invoke a
callback for every membership probe. They now share a small predicate factory
with an indexed, short-circuit loop over one retained inventory. Metadata support
reuses those predicates. This preserves store order and performs no memoization
of mutable values or user protocol results.

The final phase10 suite passes **1,377 tests, zero failures, 106 skipped**. Strict
Clippy, formatting and all three direct Node workflows pass. The complete cold
rerun, one job at a time with no overlapping tests/builds/profiling, is:

| Build | Wall seconds | Peak RSS bytes | Output |
| --- | ---: | ---: | --- |
| light-cljs render | 30.66 | 563,331,072 | 12,617,718 bytes |
| light-cljs main none | 261.18 | 1,418,936,320 | 33,583,732 bytes |
| light-cljs main advanced | 267.48 | 1,472,495,616 | 25,663,279 bytes |
| shadow-cljs release main | 62.09 | 4,086,693,888 | 1,176 files, 916 compiled, zero warnings |
| shadow-cljs compile main | 33.02 | 2,430,648,320 | 1,209 files, 1,189 compiled, zero warnings |

The none build is 7.88 times faster than the original 2,057.49 seconds. Against
this fresh comparison, advanced remains **4.31 times slower** than shadow release
at **36% of its RSS**; none remains **7.91 times slower** than shadow dev at
**58% of its RSS**. The requested speed goal has not been reached. Scope and
unverified application execution caveats remain unchanged.

All three application bodies match phase6b byte for byte after excluding their
embedded runtimes; all complete outputs pass `node --check`. The port fingerprint
remains unchanged. Logs, output checks, binary/patch hashes and the port hash are
in `results/compiler-2026-10-16/penpot-phase10/`. The executable, complete patch,
untracked name-index source and generated JavaScript remain in
`/private/tmp/light-cljs-phase10/`.

### Native sequence cursors (phase11)

`first`, `rest` and `next` no longer redispatch ISeqable when handed an existing
native sequence. IndexedSeq and NodeSeq next cursors advance directly without
calling an overridden ISeq rest. This matches the pinned upstream definitions;
explicit ISeq and INext overrides still dispatch. The regression runs at every
optimization level and inside a source macro. A 100,000-entry QuickJS traversal
probe fell from 3,862 to 1,853 ms. The full suite passes 1,378 tests, zero failures,
106 ignored; strict Clippy, formatting and the three direct Node workflows pass.

The readable runtime grew by 767 bytes. Eight advanced fixture ceilings exceeded
their previous limits by 11–362 bytes and were raised to the next 1,000 bytes,
with the correctness/performance rationale recorded beside the fixtures for the
eventual commit message.

| Build | Wall seconds | Peak RSS bytes | Output bytes |
| --- | ---: | ---: | ---: |
| light-cljs render | 27.32 | 570,523,648 | 12,618,485 |
| light-cljs main none | 214.90 | 1,421,312,000 | 33,584,499 |
| light-cljs main advanced | 227.37 | 1,497,202,688 | 25,664,046 |

These are sequential cold processes with no overlapping CPU-heavy work. Against
the unchanged fresh phase10 shadow measurements, advanced remains **3.66 times
slower** at **37% of release RSS**, and none **6.51 times slower** at **58% of dev
RSS**. None is 9.57 times faster than the original 2,057.49-second measurement.
The speed goal is still unmet. All three application bodies match phase6b outside
the embedded runtime, and complete outputs pass `node --check`. Application
execution and scope caveats remain unchanged. Evidence is retained in
`results/compiler-2026-10-16/penpot-phase11/`; binaries, patch and generated
JavaScript remain in `/private/tmp/light-cljs-phase11/`.

### Scoped evaluator service lookup (phase12)

Active compiler callbacks now retain the evaluator's immutable service object
within their existing context lifetime. Getting a service function and checking
the bridge no longer reparses a JavaScript expression on each nested operation.
This introduces no Persistent roots and does not retain macro inputs or change
transported data. All 1,378 tests pass, strict Clippy and formatting pass.

The cold render is 26.06 seconds / 566,476,800 bytes RSS, with a byte-identical
complete output to phase11. No CPU-heavy work overlapped it. This intermediate
step did not rerun main. Evidence is in
`results/compiler-2026-10-16/penpot-phase12/`.

### Protocol selection and fresh profile (phase13)

Public protocol consumers now keep their initial membership decision while
selecting the method, avoiding a redundant satisfies? call. A method getter that
removes its marker can no longer turn a missing method into a missing-key
fallback. Explicit returned methods, native implementations and missing-method
errors remain covered at every optimization level and inside source macros.
All 1,379 tests pass, strict Clippy and all three Node workflows pass.

A cold render measured 24.93 seconds / 558,809,088 bytes RSS. The following
instrumented main measured 190.08 seconds / 1,394,130,944 bytes RSS; it is a
profile rather than a clean main benchmark. Both application bodies match
phase6b outside the runtime, and complete outputs pass node --check.

The updated profile still attributes 29.84 seconds to environment decoding,
13.19 to form import, 11.12 to argument import, 8.09 to form export, 9.73 to unit
evaluation, 7.43 to diagnostics and 24.32 to other analysis. Reader preparation
is down to 15.30 seconds (11.94 for aliases), included within macro-call times.
Instaparse's grammar macro takes 16.57 seconds; 1,033 rumext defc calls take
20.54 seconds. Deduplicated AST table storage is 284.16 MB, down from phase7's
470.47 MB, excluding payload allocations. Evidence and profiling limitations
are in `results/compiler-2026-10-16/penpot-phase13/`.

## Default V8 regression check (October 27, 2026)

The slowdown reproduces without statistics or source maps: **71.76 / 21.18 s =
3.39x slower** with the original `benchmarks/penpot.bb` main-none command.
Three quiet-machine runs per cell, independent medians, wall range in brackets;
CPU time is seconds and RSS is decimal GB. `maps` includes source content.

| Revision | Flags | Wall, s | User CPU, s | Peak RSS, GB | Pre-run loads (1/5/15 min), rounds 1; 2; 3 |
| --- | --- | ---: | ---: | ---: | --- |
| `b51af6a6` (October 17 promotion) | none | 21.18 [21.17–21.40] | 25.04 | 0.978 | 2.37/2.29/2.25; 2.14/2.27/2.26; 2.27/2.29/2.26 |
| `045c5926` | none | 71.76 [71.55–72.52] | 73.81 | 0.843 | 2.11/2.24/2.23; 2.08/2.28/2.26; 2.04/2.23/2.24 |
| `045c5926` | stats | 71.51 [71.14–72.79] | 73.62 | 0.847 | 2.25/2.26/2.25; 1.87/2.21/2.24; 2.52/2.28/2.26 |
| `b51af6a6` | maps | 21.85 [21.69–21.85] | 25.63 | 1.221 | 2.62/2.37/2.28; 2.02/2.19/2.23; 3.02/2.47/2.33 |
| `045c5926` | maps | 72.41 [71.87–72.72] | 74.45 | 1.058 | 2.22/2.29/2.26; 2.29/2.23/2.24; 2.85/2.47/2.33 |
| `045c5926` | stats, maps | 72.31 [71.79–73.23] | 74.44 | 1.075 | 2.30/2.31/2.26; 1.95/2.16/2.21; 2.49/2.43/2.33 |

Maps-enabled builds regress 3.31x with identical flags at both revisions.
The statistics/map differences cannot explain either slowdown. Every repeated
revision/condition produced byte-identical JavaScript.

The compiler binaries were rebuilt at both revisions with default release
features: `b51af6a656c31554ca80a823f0922054355b0a18` and
`045c592656afc9d623ff73530847c782f6905099`.
[The generated-project check](generated-shadow-vs-light.md#default-v8-regression-check-october-27-2026)
records the shared machine, toolchain and build isolation. The five read-only
Penpot roots still match the October 17 fingerprint: 996 files,
`b36daf80aa370322598f53f5bf776391036ab70cf993835793daa1285015ba48`.
No research files were changed. `/usr/bin/time -l` measures cold compiler
processes, retaining OS file caches. Emitted modules pass Node syntax checks;
Penpot was not bundled or run in a browser.

The October 17 revision predates `--no-cache` and `--stats`: its original direct
compile has no persistent compiler cache, and a `--stats` probe exits with usage.
Current runs add `--no-cache`; other command arguments are identical. The
historical V8 build accepts an explicit patched archive/bindings pair from the
machine's release cache (its patch and bindings are unchanged), because its
build script predates automatic machine-cache lookup. No engine was rebuilt.

```sh
PORT=/Users/admin/src/light-cljs/research/penpot-light
/usr/bin/time -l "$BIN" --compile --esm \
  --macro-expansions 100000000 --macro-nodes 4000000000 \
  --macro-timeout-ms 3600000 --macro-memory-limit 4000000000 \
  --macro-poll-budget 1000000000000 \
  --source-root "$PORT/frontend/src" --source-root "$PORT/common/src" \
  --source-root "$PORT/vendor" --source-root "$PORT/generated" \
  --source-root "$PORT/shims" --entry app.main --optimize none
```

Add `--no-cache` at the current revision. Independently toggle `--stats` and
`--source-map "$MAP" --source-map-include-sources`, using one fixed absolute
map path under the worker's `target/`. The baseline supports the map flags;
its statistics combinations are unavailable. Revision order alternates as in
the generated-project check.

### Culprit, repair and three-run verification

The [scripted generated-project bisect and both profiles](generated-shadow-vs-light.md#culprit-and-profiles)
identify `b49764765e7f07cf7749ceed87e7fc48e37e5969`: it constructed a native
multimethod argument index on every emitter, scanning shared expression arenas
again when each completed unit was prepared. Its only consumer is native
emission. The separate Penpot profile has 46,774 of 57,747 main-thread samples
(81.0%) in the merge-front → release IR branch, through `Emitter::new` and arena
iteration/indexing. That instrumented 80.036 s run is excluded from the medians.

Fix `db78b7bbc0b9b2d7577ec907f70c5ed3806d8526` builds the index only in the
existing native-emission branch. The culprit's parent and repaired default
compiler were built and timed sequentially, with the same commands and port.
Three runs per cell, independent medians, wall range in brackets:

| Revision | Flags | Wall, s | User CPU, s | Peak RSS, GB | Pre-run loads (1/5/15 min), rounds 1; 2; 3 |
| --- | --- | ---: | ---: | ---: | --- |
| parent `c92e5a55` | none | 11.51 [11.50–11.58] | 13.56 | 0.860 | 2.21/2.42/2.41; 1.94/2.35/2.39; 1.82/2.29/2.36 |
| fixed `db78b7bb` | none | 11.50 [11.50–11.60] | 13.56 | 0.846 | 2.21/2.28/2.34; 2.06/2.21/2.30; 2.02/2.17/2.28 |
| fixed `db78b7bb` | stats | 11.64 [11.63–11.66] | 13.66 | 0.866 | 2.10/2.25/2.33; 2.20/2.23/2.31; 1.88/2.13/2.26 |
| fixed `db78b7bb` | maps | 12.05 [12.01–12.06] | 14.07 | 1.092 | 2.22/2.27/2.33; 2.18/2.22/2.31; 1.82/2.11/2.25 |
| fixed `db78b7bb` | stats, maps | 12.13 [12.05–12.20] | 14.12 | 1.069 | 2.03/2.23/2.32; 2.22/2.23/2.31; 2.29/2.20/2.28 |

The original-command median falls from **71.76 to 11.50 s**, matching the
culprit's parent (11.51 s) and below the remeasured October baseline (21.18 s).
The matched direct+stats runs' median `trace.release-ms` falls from 58.577 to
**0.238 s**. All repaired and parent outputs are byte-identical to the original
`045c5926` output for the same flag combination and pass Node syntax checking.
The regression is in compiler bookkeeping; these measurements cover uncached
`app.main --optimize none` at the default one job.

[Compiler verification](generated-shadow-vs-light.md#repair-and-three-run-verification)
records 2,132 default tests and three native multimethod tests passing,
formatter/Clippy results, the restored temporary Wasm workspace isolation and
the independently reproduced engine-free/engine-lint baseline limitations.
