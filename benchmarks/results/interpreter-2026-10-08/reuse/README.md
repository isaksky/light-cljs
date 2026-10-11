> **Archived native measurement.** The interpreter, benchmark harnesses and runner
> scripts have been removed. Commands and implementation claims below describe
> the historical revision recorded here, not the current checkout. See
> [the archive contract](../../../interpreter.md).

# Bounded reuse and scalar-root comparison

Compared with the immutable [known-good native baseline](../README.md), using
its saved binaries and a hash-verified rebuild of its exact source archive.
The same M3 Ultra VM/toolchain and [measurement boundaries](../../../interpreter.md)
apply. No production dependencies, frontend implementation changes, disk cache,
parallel evaluator or skipped expansion were introduced.

## Removed work

Immediate nil, boolean, numeric, character and static-native values now live
inline in owned handles. They no longer allocate an Rc root plus weak registry
entry. Owner checks and semantic allocation charges are unchanged. This
particularly reduces arithmetic-loop allocations and subsequent weak-root GC.
Traced values still share root records. Larger inline handles can increase the
bytes needed for argument/value buffers; fewer allocations do not imply fewer
bytes for every workload.

Analysis reuses cleared, bounded constant/provenance/Var buffers. A context-local
FIFO additionally reuses complete immutable analyzed units for the same retained
Form when no macro or definition analysis occurred. Keys include exact Form and
namespace identities, shared Var/namespace revisions and configuration revision.
Code retains bodies, constants, Var IDs, permissions and source provenance;
execution still reads live roots and checks permissions. Required module
initialization remains in the existing namespace loader, whose loaded state
prevents repeat analysis/effects. Reload and failed-load recovery still execute
normal reading, expansion and initialization.

The cache holds at most 32 entries, 8,192 semantic/executable nodes and 256 KiB
of conservatively counted source text, with 64 KiB per source. Weak Form keys
release dead inputs at the next analysis/collection. These structural bounds
are not an absolute transitive heap-byte limit. Supplied data and Vars can
reference larger graphs. Active finite fuel/allocation limits, reduced analysis
depth, execution polling and cleanup bypass optional analysis reuse. Definitions
and **all** macros, including builtins, remain uncached. The
[API contract](../../../../docs/interpreter.md#bounded-analysis-reuse) explains
invalidation, counters, manual clearing and scratch retention.

## Repeated saved-binary measurements

Two complete runs, each with nine samples and 32 operations per resident batch.
The first run overlapped part of the debug test suite; the second was run after
compiler/frontend validation with no task-owned tests or builds active during
measurement. Both raw runs are retained. Numbers below use run 2, in
**microseconds per operation**, median ± MAD. GC is measured separately after
32 evaluations and amortized by 32. MAD describes variation, not confidence.

| Operation | Baseline | Current |
| --- | ---: | ---: |
| Closure/loop evaluation | 387.78 ± 1.99 | 333.64 ± 7.26 |
| Collection evaluation | 951.58 ± 8.47 | 950.94 ± 6.67 |
| Lazy evaluation | 502.27 ± 3.12 | 483.52 ± 2.26 |
| Closure/loop GC per eval | 227.70 ± 7.22 | 12.87 ± 0.51 |
| Collection GC per eval | 597.39 ± 10.05 | 429.01 ± 5.26 |
| Lazy GC per eval | 335.68 ± 5.11 | 203.74 ± 5.22 |
| Closure/loop read/analyze/eval | 542.77 ± 6.26 | 479.94 ± 3.01 |
| Collection read/analyze/eval | 1146.07 ± 10.48 | 1142.20 ± 14.07 |
| Lazy read/analyze/eval | 602.08 ± 3.42 | 583.46 ± 6.84 |

Across the two runs, eval median changes were −16.7%/−14.0% for closures,
+1.1%/−0.1% for collections and −2.6%/−3.7% for lazy sequences. GC changes were
−94.6%/−94.3%, −27.1%/−28.2% and −38.4%/−39.3% respectively. Collection execution
is effectively flat in these samples; the GC/allocation result is clearer.
Embedding invoke changed −5.8%/−9.1%, to 31.41 µs in run 2. First resident module
load varied +2.3%/−6.1%; cached require varied +4.9%/+0.1%, to 24.21 µs in run 2.
There is no claimed general module-load or cached-require speedup.

These fixtures contain macros, so their analysis remains uncached. Run 2
analysis medians were 124.78 → 124.95 µs, 160.82 → 160.64 µs and
80.75 → 79.21 µs. That is broadly unchanged; scalar-root allocation removal
and scratch reuse do not eliminate required macro work.

| Warm operation | Allocations/op before → after | Requested bytes/op before → after |
| --- | ---: | ---: |
| Closure/loop eval | 6666 → 514 | 266624 → 20544 |
| Collection eval | 20756 → 16121 | 1039400 → 1053216 |
| Lazy eval | 10658 → 7571 | 523284 → 518748 |
| Embedding invoke | 583 → 1 | 23320 → 40 |
| Cached require | 422 → 410 | 19852 → 19284 |
| First module load | 58942 → 47476 | 4443293 → 3922477 |

Allocation counts/layout bytes come from separate instrumented runs. Values are
medians, including first-sample backing-buffer growth. Collection requested
bytes increase 1.3% despite fewer allocations because rooted handles are wider.
Median whole-process live requested MiB after eval/after GC change from
10.68/2.54 to 1.67/1.04 for closures, 39.97/19.56 to 32.31/17.57 for collections,
and 19.38/9.78 to 15.62/9.79 for lazy sequences. This is allocator layout
bookkeeping, not RSS; retained backing capacities matter.

Fresh-process medians ± MAD, in **milliseconds**, include file reads, usable
result, printing and teardown. OS caches are uncontrolled:

| Fixture | Baseline | Current |
| --- | ---: | ---: |
| Closure/loop | 3.397 ± 0.219 | 3.183 ± 0.035 |
| Collections | 4.276 ± 0.115 | 4.176 ± 0.137 |
| Lazy pipeline | 3.466 ± 0.044 | 3.405 ± 0.065 |
| Three-module report | 6.926 ± 0.233 | 6.079 ± 0.154 |

Cold-process changes across the two runs were −4.7%/−6.3%, −2.3%/−2.3%,
+2.4%/−1.7% and −7.7%/−12.2%. Do not infer cold-disk behavior or universal speedup.
[Run 1](run-1/comparison.json), [run 2](run-2/comparison.json),
[distributions](run-2/summary.json), [raw resident samples](run-2/current-interpreter.csv)
and [allocation samples](run-2/current-allocations.csv) retain the complete data.

## Isolated analysis reuse

The exact same supplemental Rust harness was linked separately to a rebuilt
baseline rlib and the current rlib. It uses only APIs present in the baseline.
Two rounds reverse baseline/current order; each round contains nine samples of
256 operations. The macro-free expression has a captured closure and lexical
locals. Cold analysis cycles through 256 distinct pre-read Forms, exceeding the
cache capacity; parsing is outside the timer. Warm analysis uses the same Form.

| Analysis | Baseline median | Current median | Allocations/op before → after |
| --- | ---: | ---: | ---: |
| Cold, distinct pre-read Forms | 24.91 µs | 24.46 µs | 525 → 519 |
| Warm, one retained Form | 24.72 µs | 0.117 µs | 525 → 3 |

Warm hits eliminate name resolution, instruction/body reconstruction and
provenance/constant construction. Cold allocation savings are small and the
time difference is not a broad analysis-speed claim. This cache does not help
freshly reparsed `eval_string` inputs or macro-bearing forms. The harness
checks expression counts and execution returns 42. Raw timings, separate
allocation samples, rlib/executable/harness identities and corpus observations
are in [semantic-comparison](semantic-comparison/identity.json).

## Behavior and final validation

The identical corpus transport, linked against baseline/current libraries,
produced **byte-identical observations for all 2,087 rows**: values, effects,
stdout/stderr, and error phases/categories. Both observation files and their
SHA-256 are retained. The transport uses finite safety budgets, so focused tests
separately exercise cache hits, effectful macros, Var/macro changes shared across
forks, namespace binding restoration, alias changes, readers/features, permissions,
resource bypass, definition `*file*` capture, loaded-module generation counts,
reload/failure recovery, dead input eviction, and failed-analysis root cleanup.

The final sources passed **418 tests in debug and release**, with ten optional
external checks separately selected below; strict all-target/all-feature Clippy
and formatting passed. All seven compiler comparisons passed against both
ClojureScript 1.12.134 and checkout `a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e`.
The lexer/parser corpus checks both passed (296 files, 2,834,491 bytes).
The explicitly selected pinned SCI/JVM reference job also passed all 2,087
oracle rows and its native/protocol/provenance checks; [full log](sci.log).

Frontend implementation is unchanged in this optimization. Run 2 parser
medians relative to the native baseline range from −1.3% to +2.3%; real-source
changes are +0.5% and +2.3%, short atoms +0.9%. Run 1 had larger transient
outliers, including +24.3% for core.cljs, not reproduced in run 2. The earlier
short-atom regression versus original Git HEAD remains documented in the
[baseline report](../README.md#frontend-regression-check); this change does not
claim to fix it.

## Reproduction and immutable evidence

```sh
python3 benchmarks/run_interpreter.py \
  --output "$HOME/.cache/light-cljs-baselines/my-reuse-run" \
  --compare "$HOME/.cache/light-cljs-baselines/light-cljs-44/verified-baseline" \
  --samples 9 --iterations 32 --frontend-ms 100 \
  --frontend-source research/clojurescript/src/main/cljs/cljs/core.cljs \
  --frontend-source research/clojurescript/src/main/clojure/cljs/analyzer.cljc
python3 benchmarks/compare_interpreter_reuse.py \
  --baseline "$HOME/.cache/light-cljs-baselines/light-cljs-44/verified-baseline" \
  --output "$HOME/.cache/light-cljs-baselines/my-reuse-semantics"
```

Original runs, copied binaries and exact archived source identities remain at
`/Users/admin/.cache/light-cljs-baselines/light-cljs-45/{run-1,run-2,semantic-comparison}`.
The comparison script extracts to its new output directory, verifies every
baseline source hash before building, and leaves the saved baseline untouched.
No Git commit identity alone is used to describe these accumulated uncommitted
sources. Reports/documentation can be updated after measurement; the manifests
identify the precise measured implementation.
