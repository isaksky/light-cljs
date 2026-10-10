# Native interpreter regression measurements

The release driver measures fresh native CLI processes separately from resident
contexts, analyzed code and callable reuse. It checks every language result and
saves raw samples, medians, median absolute deviation (MAD), min/max, source and
fixture SHA-256 identities, machine/toolchain information and executable copies.
There is no SCI/JVM speed comparison or speedup target.

## Reproduce and retain a baseline

Python 3.11+ and the Rust toolchain are sufficient. No benchmark dependency,
JVM, JavaScript engine or network service is used. Run from the repository:

```sh
python3 benchmarks/run_interpreter.py \
  --output "$HOME/.cache/light-cljs-baselines/my-baseline" \
  --samples 9 --iterations 32 --frontend-ms 100

# Use the same fixtures and frontend-source options when comparing.
python3 benchmarks/run_interpreter.py \
  --output "$HOME/.cache/light-cljs-baselines/my-comparison" \
  --compare "$HOME/.cache/light-cljs-baselines/my-baseline" \
  --samples 9 --iterations 32 --frontend-ms 100
```

Output directories must be new and outside `target`. The driver builds release
CLI, interpreter, tokenizer and parser executables, then copies them into
`bin/` before any feature rebuild can replace them. A separate
`interpreter-allocations` binary enables the benchmark-only allocator. Keep the
directory: `cargo clean` will not remove these executables. The source archive
includes current uncommitted sources, tests, fixtures and examples; the Git HEAD
alone does **not** identify a build made in a dirty checkout. `manifest.json`
identifies the saved source files and executables. A saved binary must run on a
compatible OS/architecture. The driver verifies baseline hashes and rejects
changed fixtures, frontend inputs or language outcomes.

For the optional real frontend inputs, append both of these to **each** command:

```sh
--frontend-source research/clojurescript/src/main/cljs/cljs/core.cljs \
--frontend-source research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

These files are hashed in the manifest and must already exist. Their absence
does not prevent the default synthetic frontend suite. Existing
`benches/{tokenizer,parser,interpreter_gc}.rs` remain available unchanged.
The GC harness separately covers cyclic workloads at different collection
cadences; see [the reclamation contract](../docs/interpreter.md).

For individual measurements:

```sh
cargo bench --bench interpreter -- --samples 9 --iterations 32
cargo bench --bench interpreter --features count-allocations -- \
  --allocations --samples 9 --iterations 32
cargo bench --bench interpreter -- --diagnostics --samples 1 --iterations 2
```

Stdout is raw CSV; diagnostic reports go to stderr. Allocation runs set
`elapsed_ns` to zero so their instrumentation overhead cannot be mistaken for
ordinary execution speed. Diagnostic timing is also a separate run.

## What the phases include

All execution is single-threaded. There is no CPU affinity or frequency control.
Release defaults are Cargo opt-level 3, without LTO or CPU tuning; the saved
environment records explicit build overrides. No compiler/build work is timed.

| Measurement | Boundary and retained state |
| --- | --- |
| Fresh process | Parent monotonic clock around spawning the native CLI, file reads, option/context setup, required module reads, parsing/lowering/analysis/evaluation, result printing, pipe drain and process exit. Every sample gets a new process/context. First samples are retained. OS file caches are uncontrolled: this is **not** a cold-disk/cache measurement. |
| Constructor / namespace initialization | Create and drop a context; the second row also initializes the namespace registry. Contexts install many builtins on demand, so this does not mean all library Vars are eagerly installed. These microbenchmarks are primed first. |
| Parse | Existing syntax parser on resident text, including tree construction/drop; no semantic lowering. |
| Lower | Semantic lowering of one pre-parsed root from the exact resident source. The phase fixtures contain no discarded forms; direct lowering does not validate discarded subtrees removed by `parse`. |
| Read | Ordinary pull reader, including parsing, lowering and EOF check. It is an alternative to `parse` + `lower`, not an additional stage to add to them. |
| Analyze | Analyze the same retained form each time, including required macro expansion and owned code generation. Expanded/generated forms are not cached. Temporary code handles drop inside the batch; arena reclamation is separate. |
| Eval | Repeated execution of one rooted analyzed form in a primed context. No reading, expansion or analysis. |
| Read/analyze/eval | `eval_source` on the retained named source; includes incremental reading and ordinary top-level semantics. This is not a sum inferred from isolated phase clocks. |
| First load | Fresh context with an explicit in-memory source provider. Time includes require, module-source copying, analysis, initialization and computation of the report result. Context/provider/main-source setup is outside this timer; filesystem loading is in the fresh-process row. |
| Cached require | Re-evaluate the main source in the already-loaded context; verifies the same result while reusing loaded namespace state. Includes reading/analysis of the caller, not module reload. |
| Embedding invoke | Repeated `Context::invoke` of an already-created closure with a retained argument. Its loop body is analyzed once. |
| GC after phase | One explicit collection after the batch. Source/form/code roots needed by the next batch stay live. This cost is reported separately, not hidden in execution; ordinary evaluation here does not automatically collect. |

Each resident sample uses 32 operations, with explicit GC outside the timed
batch and another separately timed GC after it. Source, form, code, caches and
scratch are warmed by initial setup/evaluation. Results pass through
`black_box`; integer extraction/checksum and normal API result handling are
included; expected-checksum assertions and CSV output are outside the batch.
Sub-microsecond and small parse phases have more relative timing variation.
Frontend harnesses retain their existing calibration and 100 ms sample targets.
Comparison CLI samples alternate baseline/current order; resident binary runs
are sequential. MAD/ranges describe observed variation, not confidence intervals.

The fixtures are independently authored, committed programs: a captured
closure/512-step loop, a 256-element map/lookup/reduction workload, a bounded
consumer of an infinite lazy pipeline, a repeatedly invoked closure, and a
three-module report that builds 256 records, aggregates eight regions and uses
a user macro. They exercise useful paths but are not a large production-library
load or a claim about every supported program. Exact results are 134400,
2763520, 2796160, 85355 and 62182 respectively.

## Allocation and live-memory interpretation

The separate counting allocator forwards unchanged requests to `System`.
Allocation counts exclude reallocations; requested bytes include each complete
new realloc size. The live byte count subtracts deallocated/old realloc layouts;
peak is the maximum live requested bytes during that region. These are Rust
allocation layout bytes for the **whole benchmark process**, including retained
fixtures and harness storage. They are not RSS, allocator usable size, reserved
pages or an OS memory limit. Only counts inside the measured region contribute
to allocation/requested-byte totals; live bookkeeping runs throughout the
instrumented process. Zero byte fields in ordinary timing CSV mean unmeasured.

Every row also records interpreter heap/name/source/code/namespace/Var/root and
arena-slot counts. The GC row shows retained logical records. Slot/backing and
scratch capacity can remain high after live objects fall; that is different
from a language object leak. Compare equivalent workloads/cadences and use the
existing cyclic GC benchmark for reclamation invariants.

## Optional source attribution

`Context::set_diagnostics(true)` enables macro and code-generation reporting;
`set_macro_diagnostics` remains available for macro-only instrumentation.
`macro_expansion_stats()` identifies the fully qualified macro and its
originating interpreted fn/letfn source form. Registered native/composed
callables can have no source definition; their canonical name still identifies
them. Redefinitions with different source text are separate rows, even at the
same filename/offset. Counters include calls, generated semantic node counts
(including metadata, for analysis expansion), and invocation plus successful
realization/lowering time. Explicit data `macroexpand` calls count invocations,
but do not generate analyzed semantic nodes.

`code_generation_stats()` identifies the original source/form span and macro
origin, successful analyzed units, expression/function output counts and
inclusive analysis time. `generations - 1` exposes repeated generation of that
source form. Distinct generated forms can share one source span/origin and
contribute to that same row. It does **not** establish equivalent output or
justify caching effectful macros. Repeated execution/invocation of retained code does not add
generation records. Nested clocks can overlap; do not sum these reports as
exclusive CPU categories. They are attribution aids, not an allocation profiler.

Reports own exact source text/location snapshots and retain no interpreter
roots. Disabling stops new measurements; existing reports remain available.
`clear_diagnostics()` clears them without changing switches; context reset
drops them and disables diagnostics. The default path does not read diagnostic
clocks, copy source snapshots or search report tables. Enabled diagnostics can
be expensive and accumulate unique locations until cleared. They do not print
anything themselves, change language effects or skip required macro expansion.
Focused tests compare output, effects, errors and live records with diagnostics
on/off, repeated generation, redefinition and disabling inside a host macro.

## Recorded native baseline

The accompanying October 8, 2026 report and raw data are in
[`results/interpreter-2026-10-08`](results/interpreter-2026-10-08/).
Measurements are from an Apple M3 Ultra virtual machine, 24 visible CPUs,
32 GiB RAM, macOS 15.7.3, Rust/Cargo 1.99.0, LLVM 23.1.2,
`aarch64-apple-darwin`. See that directory's report for measured bottlenecks,
frontend comparisons, variation and the saved known-good executable location.

## Bounded reuse follow-up

The [bounded-reuse report](results/interpreter-2026-10-08/reuse/README.md) records
two saved-baseline comparisons, scalar-root allocation/GC changes, and the exact
scope of the Form-identity cache. `compare_interpreter_reuse.py` rebuilds a
hash-verified baseline archive, links identical analysis/observation harnesses
to both libraries, compares all corpus observations, and measures cold distinct
Forms separately from repeated analysis of one retained Form. It does not
assume macros or repeated source strings are safe to cache.

## Persistent `into` with a custom transducer

The `into_lazy` and `into_transducer` rows in `benches/interpreter.rs` compare
`(into [] (map inc (range 128)))` with three-argument `into` using an ordinary
interpreted increment adapter. Each operation constructs its source and result,
then sums the vector to verify 8256. Both retain analyzed code, and both keep GC
outside the timed evaluation batch; the adapter is constructed during evaluation.
The normal benchmark commands above reproduce both rows without optional tools.

On 2026-10-08, macOS arm64 with Rust 1.99.0, nine release samples of 32 operations
produced the following per-operation medians. Timing was repeated after other
builds/tests finished. Allocation instrumentation ran separately. These are a
small workload comparison, not a general speedup claim.

| Workload | Eval median (µs) | MAD (µs) | Range (µs) | Allocations | Reallocations | Requested bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Lazy mapping, two-argument `into` | 238.02 | 7.50 | 229.80–256.14 | 3672 | 265 | 391016 |
| Custom transducer, three-argument `into` | 193.96 | 3.70 | 182.82–197.67 | 2394 | 137 | 301476 |

The custom transducer took 18.5% less median evaluation time in this run, with
34.8% fewer allocations and 22.9% fewer requested bytes. Persistent destination
updates remain the same; avoiding the intermediate lazy mapping nodes offsets
ordinary interpreted adapter dispatch here. Raw batch totals, individual samples,
and separately measured GC rows are retained in
[`into/timings.csv`](results/interpreter-2026-10-08/into/timings.csv) and
[`into/allocations.csv`](results/interpreter-2026-10-08/into/allocations.csv).


## Map/filter/remove adapter regression check

The existing harness passed with nine samples and 32 iterations after adding
native `map`/`filter`/`remove` transducer constructors. The first timing run
overlapped strict Clippy and was slower across all measured evaluation paths;
the retained repeat ran after compilation and native verification completed.
Relative to the preceding `into` measurements, repeated lazy-into evaluation
was 235.92 µs (MAD 6.38, range 219.27–258.47) versus 238.02 µs, and the existing
custom-transducer evaluation was 188.93 µs (MAD 3.62, range 182.40–203.24) versus
193.96 µs. These samples show no dispatch regression beyond their variation;
they are not a controlled speedup claim. Both workloads retained exactly the
same per-sample allocation, reallocation and requested-byte counts as the
preceding run, including initial capacity growth.

The complete existing harness's [initial timings](results/interpreter-2026-10-08/map-filter-remove/timings.csv),
[repeat timings](results/interpreter-2026-10-08/map-filter-remove/timings-repeat.csv),
[allocation samples](results/interpreter-2026-10-08/map-filter-remove/allocations.csv)
and [source/toolchain identities and summary](results/interpreter-2026-10-08/map-filter-remove/identity-and-summary.json)
are retained. These are regression measurements of existing paths; dedicated
native transform comparisons belong to the focused transducer benchmark work.

## Bounded native transducer comparison

`bounded_lazy`, `bounded_transducer` and `bounded_eager` all filter odd values
from the finite input `(range 256)`, increment selected values, take the first
64 and sum the resulting vector to 4160. The lazy path uses collection arities;
the native transform path composes `(filter odd?)`, `(map inc)` and `(take 64)`;
the eager path materializes `filterv`/`mapv` results before taking 64. Eager
materialization processes the whole finite input, while the other two can stop
early. Each fixture asserts the same result in every benchmark phase.

After adding take/drop/take-while, the full harness passed in separate timing
and allocation runs with nine samples and 32 iterations. Evaluation-only
measurements on the recorded macOS arm64 host were:

| Pipeline | Median µs | MAD µs | Range µs | Allocations | Reallocations | Requested bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Lazy filter/map/take | 253.73 | 1.18 | 250.29–257.51 | 3929 | 199 | 337124 |
| Native filter/map/take transducer | 180.32 | 2.52 | 171.88–187.41 | 2482 | 71 | 248616 |
| Eager filterv/mapv, then take | 259.95 | 1.22 | 254.94–272.14 | 3517 | 135 | 367072 |

Allocation and requested-byte counts are medians per evaluation; instrumentation
runs report no timing. These are workload-specific samples, without CPU affinity
or frequency control, and do not imply a required speedup for other pipelines.
Timing ran after verification and all benchmark compilation completed. Raw
[timing samples](results/interpreter-2026-10-08/take-drop-take-while/timings.csv),
[allocation samples](results/interpreter-2026-10-08/take-drop-take-while/allocations.csv)
and [source/toolchain identities and summary](results/interpreter-2026-10-08/take-drop-take-while/identity-and-summary.json)
are retained for reproduction.

## Native cat and mapcat, 2026-10-08

Nine release samples of 32 analyzed evaluations compare complete and early
consumption of 128 pairs `[x x]` for `x` from 0 through 127. Complete scenarios
produce the validated sum 16,256; early scenarios stop after 64 flattened values
and produce 992. `mapcat_lazy` reduces existing collection-taking mapcat;
`cat_lazy` reduces apply/concat over lazy map. Their corresponding transducer
scenarios use the same mapper and inputs, with `(take 64)` for early termination.
Collection-taking mapcat/concat retain their ordinary lookahead. Setup,
parsing/analysis and garbage collection are outside the resident evaluation
samples; all phases and checksums remain in the raw files.

| Scenario | Evaluation µs, median ± MAD | Allocations/evaluation | Requested bytes/evaluation |
|---|---:|---:|---:|
| `cat_lazy_complete` | 444.45 ± 4.56 | 7,755 | 490,072 |
| `cat_xf_complete` | 398.74 ± 1.83 | 6,687 | 453,624 |
| `cat_lazy_early` | 152.67 ± 2.49 | 2,626 | 164,920 |
| `cat_xf_early` | 116.96 ± 2.78 | 1,958 | 132,392 |
| `mapcat_lazy_complete` | 446.89 ± 2.88 | 7,757 | 490,152 |
| `mapcat_xf_complete` | 307.77 ± 11.47 | 5,156 | 353,988 |
| `mapcat_lazy_early` | 150.17 ± 2.61 | 2,628 | 165,000 |
| `mapcat_xf_early` | 96.31 ± 1.90 | 1,587 | 108,160 |

Allocation counts and bytes are medians from a separate instrumented build.
These pipelines used fewer allocations and less elapsed time with transducers;
this does not imply a universal advantage for arbitrary nested inputs or callback
costs. Timing and allocation runs were sequential after verification and both
benchmark builds, without CPU affinity or frequency control. The ordinary GC
benchmark also passed its live-record stabilization assertions at collection
cadences 1, 16 and 256; its uninstrumented allocation columns are placeholders.

Raw [timing samples](results/interpreter-2026-10-08/cat-mapcat/timings.csv),
[allocation samples](results/interpreter-2026-10-08/cat-mapcat/allocations.csv),
[GC output](results/interpreter-2026-10-08/cat-mapcat/gc.log) and
[source/binary identities, commands and summary](results/interpreter-2026-10-08/cat-mapcat/identity-and-summary.json)
retain reproduction details and median/MAD/range for each nested scenario.


## Native partition-all, 2026-10-08

Nine release samples of 32 resident analyzed evaluations group 128 values by
three, including a final two-item partition, and validate the total sum 8,128.
Both pipelines materialize vector groups in a vector: the lazy path maps `vec`
over collection-taking partition-all before two-argument `into`; the transducer
uses `(into [] (partition-all 3) (range 128))`. Both then sum each group and the
resulting totals with the same outer operations. Setup, parse/analysis and GC
are outside the resident evaluation samples.

| Pipeline | Evaluation µs, median ± MAD (range) | Allocations/evaluation | Reallocations/evaluation | Requested bytes/evaluation |
|---|---:|---:|---:|---:|
| Lazy partition-all with vector normalization | 376.03 ± 7.96 (358.37–396.34) | 6,062 | 136 | 436,520 |
| Partition-all transducer | 229.91 ± 4.63 (215.86–269.65) | 3,702 | 178 | 289,884 |

The transducer used fewer allocations and less median time on this workload,
with more reallocations. Allocation medians are from a separate instrumented
build; arena growth creates the larger requested-byte maxima recorded in the
raw samples. This is a measured pipeline comparison, not a promise of a speedup
or identical lazy lookahead. Final timing/allocation runs were sequential after
all builds and feature verification, without CPU affinity or frequency control.
Earlier samples that overlapped verification are retained separately and excluded
from these results.

The ordinary GC benchmark passed its stable-live-record checks. The instrumented
GC benchmark failed its existing warmed zero-allocation assertion with 512
allocations and 8,192 requested bytes over 256 collections. An untouched archive
of starting commit `fe08ac83dc067ca85344663f2d0f2a96e613827e` reproduces the
same failure, independently of the new transducers. The subsequent
[collector repair](#warmed-collector-allocation-repair) restores that invariant;
these original failure logs remain part of the partition-all evidence.

Raw [timing samples](results/interpreter-2026-10-08/partition-all/timings.csv),
[allocation samples](results/interpreter-2026-10-08/partition-all/allocations.csv),
[ordinary GC output](results/interpreter-2026-10-08/partition-all/timing-gc.log),
[instrumented GC failure](results/interpreter-2026-10-08/partition-all/allocation-gc.log),
[starting-commit reproduction](results/interpreter-2026-10-08/partition-all/baseline-allocation-gc.log)
and [identities, commands and summary](results/interpreter-2026-10-08/partition-all/identity-and-summary.json)
retain the full evidence.

### Warmed collector allocation repair

The allocation source was namespace lookup during analysis-cache pruning, before
mark/sweep. `current_namespace_id` rebuilt `Name::new(None, "*ns*")` each time;
collecting its UTF-16 name into an `Rc` slice requested two allocations totaling
32 bytes. The namespace registry now retains that immutable lookup key and
shares it with forks. Every lookup still resolves the current mapping, Var root
and dynamic binding. Tracing, allocation accounting, the benchmark's measurement
window and its zero-allocation assertion are unchanged.

The pre-repair working tree reproduced 512 allocations, zero reallocations and
8,192 requested bytes over 256 collections at cadence 1. After the repair, all
seven samples at each cadence (1, 16 and 256) report **zero collector allocations,
reallocations and requested bytes**. Ordinary and instrumented runs retain the
same 317 heap values, 320 names, one source, one executable unit, six namespaces,
314 Vars and six roots as before. Arena slots also match at each cadence:
1,432, 2,767 and 24,127 respectively. The zero columns in ordinary timing output
remain placeholders; the separate instrumented run establishes the allocation
result.

A separate ordinary run measured these median collection times (seven samples):

| Collection cadence | Before µs/collection | After µs/collection |
| --- | ---: | ---: |
| 1 | 42.38 | 42.71 |
| 16 | 307.32 | 323.02 |
| 256 | 6,244.46 | 6,401.50 |

A regression test collects with live analysis, nested namespace bindings and a
replaced `*ns*` Var mapping, then verifies cache invalidation and retained code.
The 159 focused native/protocol/provenance tests pass, including reclamation,
deferred values, collections, mutable state, transducers, ownership and limits;
the opt-in JVM reference test remains ignored. Formatting and strict all-target
Clippy pass. This is an allocation-invariant repair, without a general timing
speedup claim.

Raw [before timing](results/interpreter-2026-10-08/warmed-collector/before-timing.log),
[before allocation failure](results/interpreter-2026-10-08/warmed-collector/before-allocations.log),
[after timing](results/interpreter-2026-10-08/warmed-collector/after-timing.log),
[after allocation counts](results/interpreter-2026-10-08/warmed-collector/after-allocations.log)
and [source identities and verification](results/interpreter-2026-10-08/warmed-collector/identity-and-summary.json)
retain the evidence. Final after-runs were sequential after all verification and
builds. Initial after-runs are retained separately because their timing may
overlap verification; they are excluded from the reported timing comparison.

## Equivalent native transducer pipelines

`interpreter_transducers` measures nine paired fixtures in one executable and
validates complete scalar/vector outputs before timing. It covers an unbounded
filter/map/take scalar reduction (1001000), identity mapping, rejection of every
item, early take, doubling mapcat (999000), two nested `cat` adapters, scalar
partition reduction (500500), materialized vector partitions, and a user-written
volatile-backed indexing adapter. The two partition cases use 1001 items and
32-item groups: completion must flush the last nine items. Lazy partition groups
are converted to vectors before full-value comparison.

```sh
cargo bench --bench interpreter_transducers -- --samples 9 --iterations 32
cargo bench --bench interpreter_transducers --features count-allocations -- \
  --allocations --samples 9 --iterations 32
python3 benchmarks/run_interpreter_transducers.py \
  --output "$HOME/.cache/light-cljs-baselines/my-transducers" \
  --samples 9 --iterations 32
# Optional old-path comparison using the saved baseline's original harness:
python3 benchmarks/run_interpreter_transducers.py \
  --output "$HOME/.cache/light-cljs-baselines/my-transducers-comparison" \
  --baseline "$HOME/.cache/light-cljs-baselines/light-cljs-44/verified-baseline" \
  --samples 9 --iterations 32
```

Each fixture provides an xf factory, transducing runner, lazy runner, resident
input and independently specified full expected output. Finite inputs are
retained vectors, identically reused by both runners. The unbounded scalar pair
constructs a new `(range)` inside **both** runners on every call. Lazy transforms
are newly constructed each time; no side reuses a memoized output. The CLI
fixture includes the exact unbounded scalar expression and expects 1001000.

| Phase | Work inside the measurement |
| --- | --- |
| `construct_xf` | Invoke an already analyzed factory and drop its returned xf. |
| `apply_xf` | Apply one retained xf to a retained completing reducer, allocating fresh per-process state; drop the resulting reducer without consuming input. |
| `construct_invoke_xf` | Construct the xf, apply it and run the entire transducing process. |
| `invoke_xf` | Reuse one retained xf with a fresh transformed reducer per call; run completion and return/drop the output. |
| `invoke_lazy` | Invoke the equivalent lazy pipeline with the same retained input. |
| `eval_xf` / `eval_lazy` | Evaluate retained analyzed calls to the respective runners, excluding parse, analysis, input construction and setup. |
| `invoke_failed_xf` | Reuse an xf which fails with a partially filled partition buffer or inside nested cat reduction; each call creates a fresh lazy source and drops its error. |
| `gc_after_*` | One explicit collection after an entire batch, reported independently from execution. |

All successful outputs pass through `black_box`. The timed checksum is the
integer result or outer vector length; full recursive value validation is
outside timing. This keeps result inspection from adding vector-copy/rooting
allocations to the materialization measurements. Three initial full-output
comparisons check that reused state restarts; two batches then warm the exact
collection cadence. Every sample asserts identical warmed heap, name, source,
code, namespace, Var, root **and slot** counts after dropping outputs and
collecting. Failed invocations exercise abandoned partition buffers and cat
cursors. Intentional retained inputs, callables and analyzed code remain roots.
The allocation build additionally asserts zero allocation requests during each
warmed collection. The existing cyclic `interpreter_gc` harness also runs in
both builds with its own unchanged cadences 1/16/256.

The report runner saves at least nine raw samples, median, MAD and range for
per-operation timing/allocation fields and absolute live/peak bytes and memory
counts. It copies binaries before feature rebuilds, archives exact dirty-tree
sources, hashes fixtures and binaries, records the machine/toolchain/build
settings and measures a fresh native CLI process separately. Allocation builds
set `elapsed_ns` to zero; requested-layout allocation bytes are not RSS or
interpreter allocation-limit units. Builds and verification must finish before
timing; there is no wall-clock CI threshold or assumed transducer advantage.

Adding these fixtures changes the general driver's complete fixture manifest;
`run_interpreter.py` continues to reject comparison with older manifests. The
optional dedicated old-path comparison verifies the old source archive, all
saved binaries and every original fixture hash, and links that **unchanged old
harness** against the current library. Its original fixture set defines both
sides of that narrower comparison. It runs two timing rounds with reversed
binary order, verifies identical language outcomes, and saves separate
allocation measurements. It does not pass an expanded manifest through the
general driver's identity check or claim old binaries implement transducers.

The [October 8–9 measured report](results/interpreter-2026-10-08/transducers/README.md)
retains nine-sample paired timings, allocation/live-memory data, all-rejecting
and partition-byte counterexamples, warmed GC evidence and unchanged old-path
comparisons. Exact sources and executable copies are saved outside `target`.

### Protocol declaration and dispatch foundation (2026-10-08)

On this arm64 macOS host with rustc 1.99.0, the opt-in release test
`cargo test --locked --release --lib protocol_dispatch_measurement -- --ignored --nocapture`
compares direct `Context::invoke` of native `identity`, an interpreted identity
closure, and a protocol method selecting that same closure. Each call returns the
identical private test receiver, checked inside every iteration. Seven samples of
2,000 calls gave medians of **36 ns native**, **148 ns closure**, and **332 ns
protocol dispatch** (ranges 35–38, 144–167 and 302–345 ns). This measures the
foundation with a private Rust receiver fixture; it makes no claim about public
reify/record constructors or a dispatch cache.

Seven samples of 100 evaluations compare ordinary `defn` replacement with
same-shape `defprotocol` registration, including reading, macro expansion,
analysis and metadata publication. Medians were **51.9 µs for defn** and
**65.6 µs for defprotocol** (ranges 49.9–57.2 and 64.0–67.1 µs). The declaration
workloads have different semantics: defn replaces its closure while protocol
registration preserves the method identity and implementation table. These are
local costs, not a speedup or equivalent compiler optimization claim.

[Dispatch/registration output](results/interpreter-2026-10-08/protocol-declarations/dispatch-registration.txt),
[GC timing output](results/interpreter-2026-10-08/protocol-declarations/gc-timing.txt)
and [instrumented GC output](results/interpreter-2026-10-08/protocol-declarations/gc-allocations.txt)
retain the commands' results. `cargo bench --locked --bench interpreter_gc` and
its `--features count-allocations` variant passed all cadence/live-state checks;
warmed collections still allocated and reallocated **zero** times at each
cadence. The two newly installed core Vars account for the expected live namespace
inventory increase. Allocation instrumentation timings are separate from ordinary
timings. Descriptor cycles and private receiver/closure roots are independently
covered by protocol and reclamation tests.


## Captured reify construction and invocation (2026-10-08)

`cargo bench --locked --bench interpreter_protocols` compares a captured closure
`(fn [_ y] (+ n y))` with `(reify P (p [_ y] (+ n y)))`, where `n` is 7.
Construction invokes the precompiled factory 2,000 times per sample; invocation
uses retained values and arguments for 20,000 calls, validating 10 on every call.
Seven measured samples follow one equal-sized warmup. Parsing, analysis and GC
are outside both phases; heap/slot/root statistics must stabilize after each
collection. These are feature costs, with no speedup claim.

| Phase | Captured closure median | Reify median | Closure allocations / bytes per op | Reify allocations / bytes per op |
| --- | ---: | ---: | ---: | ---: |
| Construct | 165 ns | 828 ns | 3 / 104 | 18 / 1,028 |
| Invoke | 242 ns | 419 ns | 1 / 40 | 5 / 344 |

Reify construction includes fresh identity, captured method closure, descriptor
membership, metadata and validated implementation table. Dispatch adds the
protocol/method lookup and selected-closure invocation boundary. Allocation
counts come from a separate instrumented build and all scenarios had zero
reallocations per operation. Measurements do not assert JVM equality/hash or
performance parity.

[Timing CSV](results/interpreter-2026-10-08/reify/timing.csv),
[allocation CSV](results/interpreter-2026-10-08/reify/allocations.csv) and
[environment/commands](results/interpreter-2026-10-08/reify/environment.txt) retain
the evidence. Public reclamation tests separately verify captured receiver
cycles, method-returned closures and repeated make/drop with stable live/slot
counts. The instrumented general collector benchmark checks all three collection
cadences separately; [its output](results/interpreter-2026-10-08/reify/gc-allocations.txt)
records zero warmed collector allocations, reallocations and requested bytes.

## Immutable record data (2026-10-08)

`cargo bench --locked --bench interpreter_records` compares ordinary maps with
three-field nominal records. Seven measured samples follow one equal-sized
2,000-operation warmup. Both sides validate the complete projected value before
timing, the nominal result kind where applicable, every operation's scalar/field
checksum, and stable live/slot counts after each batch. Parsing, analysis and GC
are outside timing. These are representative feature costs, with no speedup
requirement or JVM performance claim.

| Operation | Map median | Record median | Map allocations / bytes per op | Record allocations / bytes per op |
| --- | ---: | ---: | ---: | ---: |
| construct | 2,171 ns | 1,842 ns | 46 / 2,636 | 36 / 2,240 |
| from-map | 3,144 ns | 1,321 ns | 60 / 3,528 | 25 / 1,056 |
| lookup | 580 ns | 588 ns | 9 / 352 | 9 / 352 |
| assoc | 1,114 ns | 1,051 ns | 20 / 1,136 | 20 / 1,136 |
| seq-reduce | 4,098 ns | 3,932 ns | 50 / 3,040 | 50 / 3,040 |
| key | 3,332 ns | 3,482 ns | 36 / 5,536 | 36 / 5,536 |

Construction compares a three-entry map literal with `->R`; both are called
through a compiled one-argument factory. From-map compares `(into {} m)` with
`map->R`, which shares the persistent payload and fills missing declared keys;
it does not compare with returning an existing map unchanged. Lookup reads `:x`,
assoc replaces `:x`, sequence reduction sums three entry values, and key lookup
retrieves a retained scalar using an equal separately constructed map/record key.
Collection-result measurements include a host lookup of `:x` for the checksum.
Separate instrumented runs supply allocation counts. Reallocations per operation
(map/record) were construct 0/0, from-map 1/0, lookup 0/0, assoc 0/0,
sequence/reduce 1/1, and key use 13/13.

[Timing CSV](results/interpreter-2026-10-08/records/timing.csv),
[allocation CSV](results/interpreter-2026-10-08/records/allocations.csv), and
[environment/commands](results/interpreter-2026-10-08/records/environment.txt)
retain the data. The [general interpreter benchmark](results/interpreter-2026-10-08/records/collections.txt)
passed its validated workloads with nine samples and 32 iterations. The
[instrumented GC benchmark](results/interpreter-2026-10-08/records/gc-allocations.txt)
passed all three collection cadences and recorded zero warmed collector allocations,
reallocations and bytes. Record reclamation tests separately retain/drop old
generations, constructor code, lazy fields, metadata and atom cycles, returning to
bounded warmed live counts.

## Record methods (2026-10-08)

The record benchmark now compares a retained ordinary `(fn [r] (:x r))` reading
a one-field record, a record protocol method reading its field, and a reify method
returning the equivalent captured value. Every call validates 7. Separate
construction samples invoke saved constructors/factories and validate results by
method dispatch outside timing. Seven samples follow an equal-sized warmup, with
2,000 operations per sample; parsing, analysis and GC are outside these phases.

| Operation | Median | Range | Allocations / bytes per operation |
| --- | ---: | ---: | ---: |
| Direct function reading a record field | 513 ns | 502–520 ns | 9 / 352 |
| Record protocol method reading a field | 740 ns | 667–899 ns | 13 / 656 |
| Reify protocol method reading a capture | 309 ns | 305–320 ns | 5 / 344 |
| Record instance construction | 842 ns | 766–857 ns | 16 / 896 |
| Reify instance construction | 814 ns | 779–879 ns | 18 / 988 |
| Full data-only record definition | 92.9 µs | 90.7–95.2 µs | 1,840 / 118,790 |
| Full record definition with captured method | 178.0 µs | 171.4–180.7 µs | 3,260 / 238,979 |

Definitions use 100 operations per sample and include reading, expansion, analysis
and publication. The method definition captures 3 and adds it to the field;
constructing field 4 must dispatch to 7. Instances retain a TypeId and map data,
while closures/captures live once per type descriptor. The field method performs
keyword lookup and protocol dispatch; the reify method only loads a captured
local, so these costs should not be interpreted as interchangeable implementations.
The raw direct-field construction control repeats the same record constructor and
shows normal run-to-run timing variation. These are feature costs, not a speedup
or SCI/JVM comparison.

Allocation counts come from a separate build. Calls and construction had no
reallocations; data-only/method definitions used 313/636 per operation. Live heap,
code and source counts stabilize after each definition batch; call/construction
batches also require stable slots and roots. Reclamation tests separately verify
old generation/capture cycles and nested returned closures.

[Timing CSV](results/interpreter-2026-10-08/record-methods/timing.csv),
[allocation CSV](results/interpreter-2026-10-08/record-methods/allocations.csv), and
[environment/commands](results/interpreter-2026-10-08/record-methods/environment.txt)
retain all samples. The [instrumented GC output](results/interpreter-2026-10-08/record-methods/gc-allocations.txt)
records zero warmed collector allocations, reallocations and requested bytes at
all three collection cadences.

## Record extensions (2026-10-09)

The record benchmark adds retained external methods over the same one-field
record used by direct and inline method controls. All calls return 7; the reify
control returns its captured scalar. Seven samples follow an equal warmup, with
2,000 calls per sample, and GC outside measurement. Timing was repeated after
compilation/conformance jobs finished to remove that competing workload.

| Operation | Median | Range | Allocations / bytes per operation |
| --- | ---: | ---: | ---: |
| Direct function reading the record field | 492 ns | 467–561 ns | 9 / 352 |
| Inline record protocol method | 717 ns | 666–807 ns | 13 / 656 |
| Externally supplied record protocol method | 673 ns | 659–745 ns | 13 / 656 |
| Reify method reading its capture | 276 ns | 274–286 ns | 5 / 344 |
| Full external replacement with a lexical capture | 88.4 µs | 82.3–91.3 µs | 1,502 / 114,270 |

Inline and external methods share the same dispatch and allocation costs; these
local timings do not establish a speedup. Reify loads a capture instead of looking
up a field. Replacement samples perform 100 full read/expand/analyze/publish
operations, preserve the current type descriptor, and validate field 4 plus
capture 3 equals 7. Each replacement takes 343 reallocations; retained calls take
none. Timing and allocation measurement use separate builds.

Every sample checks equivalent values and stable warmed memory. Reclamation tests
also retain old closures through replacement/GC, release replaced captures, and
drop old record generations. The instrumented collector reports zero warmed
allocations, reallocations and requested bytes at all three collection cadences.
[Timing CSV](results/interpreter-2026-10-09/record-extensions/timing.csv),
[allocation CSV](results/interpreter-2026-10-09/record-extensions/allocations.csv),
[GC output](results/interpreter-2026-10-09/record-extensions/gc-allocations.txt), and
[environment/commands](results/interpreter-2026-10-09/record-extensions/environment.txt)
preserve all samples and controls.

## Protocol and immutable record cost report

The dedicated `interpreter_types` harness completes the type-feature measurements
with equivalent scalar programs and explicit object lifetimes. The
[recorded report](results/interpreter-2026-10-09/types/README.md) preserves all raw
samples, distributions, type/content validation and source/fixture identities.
No production dependency or speculative optimization accompanies this report.

```sh
cargo bench --locked --bench interpreter_types -- --samples 9 --iterations 32
cargo bench --locked --bench interpreter_types --features count-allocations -- \
  --allocations --samples 9 --iterations 32
python3 benchmarks/run_interpreter_types.py \
  --output "$HOME/.cache/light-cljs-baselines/my-types-run" \
  --baseline "$HOME/.cache/light-cljs-baselines/light-cljs-58/transducers" \
  --samples 9 --iterations 32
```

The optional baseline must be a saved report with matching original fixtures.
The runner reuses `run_interpreter.py` build/hash/statistics helpers and
`run_interpreter_transducers.py`'s verified archived-harness A/B procedure;
it does not weaken the general driver's whole-fixture identity check. New
fixtures are compared within one binary. Old paths compare exactly the archived
harness/fixtures against both libraries and verify every scalar language outcome.
Source archives and copied binaries remain outside `target`; the report's
manifest includes the dirty source tree because Git HEAD is insufficient.

Each ordinary or allocator build has nine samples and two equal-sized warmup
batches per resident phase, with explicit GC once per batch. Time and allocator
instrumentation use separate executables; instrumented `elapsed_ns=0` is
unmeasured. Every timed result passes through `black_box`. Scalar extraction,
checksums and API result handling are included; fixture construction and
full equality/type checks are outside timing. Counts and requested/live/peak
bytes use the existing benchmark allocator and its whole-process requested
layout accounting, not RSS, allocator usable bytes or interpreter GC allocation
units. Slots include reusable vacancies and are not live-object counts.

| Phase | Measured operation and intentionally retained state |
| --- | --- |
| `invoke_1000` | Prebuild 1000 values numbered 0–999, then invoke a saved ordinary method/function value through `Context::invoke` on each value. Each operation sums to 499500. Map and direct-record functions read `:n`; inline and external record methods dispatch through saved `p`; reify captures `n`; closure objects capture `n` and a saved wrapper invokes them. `mixed` alternates two nominal types with identical method results and is reported separately. |
| `analyzed_sum_1000` | Evaluate a retained analyzed `reduce` that invokes the same saved function on the same prebuilt finite vector. The language reduction and vector traversal are included on every side; parsing, analysis and construction are excluded. This is a separate boundary from 1000 host calls. |
| `construct_1000` | Invoke the saved factory/constructor 1000 times with retained scalar inputs; drop all fresh outputs. This includes repeated reify/closure make/drop. Constructor analysis and input construction are excluded on every side; no method traversal occurs. |
| Field/key/assoc/dissoc/from-map | One invocation of a retained function on retained input; associative results are observed through `:n`. Full content and nominal record/map type are checked before timing. `map->R` is paired with copying the same map using `(into {} m)`, not an identity operation. Key inputs are equal freshly constructed values, not the identical keys retained by the lookup map. |
| `macroexpand` | Expand a retained quoted protocol, record or extension form. No reading, analysis or definition publication. |
| `analyze` | Analyze a retained form, including native macro expansion and code generation; drop the new analyzed handle. Effectful forms do not use analysis reuse. |
| `eval_definition` | Execute retained analyzed definition/extension code. Each record definition publishes a new generation; protocol redeclarations preserve shape; extension replaces its implementation. No read/analysis time. |
| `read_analyze_eval` | Re-evaluate retained named definition source, including reading, macro expansion, analysis and publication. The isolated clocks above are not additive. |
| `redefine` | Repeated successful captured record definitions, invalid duplicate-field definitions, or replacement external implementations capturing a fresh vector. Retained-generation cases keep one old record plus its type descriptor; released-generation cases keep neither. Errors/results drop before GC. |
| `release_final` | Nine separate transitions: retain an old generation if requested, redefine and collect outside timing, then drop retained handles and collect inside timing. Released live counts must exactly match the original baseline; slots keep high-water capacity. |
| `invoke_gc_callback` | One inline/external/reify method invocation that explicitly collects inside a native callback. Its one internal collection is included in operation cost and recorded in `gc_collections`; the usual post-batch collection remains separate. |
| `gc_after_*` | One separately timed collection after the batch; record heap/name/source/code/namespace/Var/root/slot counts and assert exact warmed stability. The allocator build asserts zero allocation/reallocation/requested bytes for every warmed post-batch collection. |
| Fresh CLI | A new native process/context reads a complete map, inline/external record, reify, closure or mixed-type fixture, defines callables, builds 1000 values, reduces, prints 499500 and exits. Process order rotates/reverses across samples; OS file caches are uncontrolled. |

Record/map results share scalar or projected content only within the documented
operation. Records remain nominally unequal to maps and to other record types;
these relationships and distinct-type map keys are checked outside timing.
Removing an extra field preserves a record, while removing a basis field returns
an ordinary map. `dissoc_basis` therefore measures equal remaining content with
a type transition; it does not establish semantic interchangeability. Retained
old records/descriptors deliberately preserve their implementation/capture graph;
after release, exact live-count assertions reject permanent registry/cache roots.
The latest definition and its metadata/code/source remain intentionally live.
