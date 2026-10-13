# Compiler collection costs — October 9, 2026

> Measurements below describe their recorded revisions. Native macro phases,
> `Context` APIs and native-disable environment variables are historical only:
> the interpreter and its harnesses have been removed. The current compiler
> runner uses compiled macros; its phase set omits native `expand-resolved`.
> Current evaluator measurements are described in [the evaluator contract](../docs/evaluator.md#verification-and-measurements).

The later [language integration measurement](#language-integration-october-10-2026)
records the expanded compiler/runtime snapshot separately from these collection
baseline results.

The collection runtime stays out of primitive-only output and appears once per
compilation unit. This run measured 10.708 µs to compile the nested collection
fixture from resident source, 2.311 ms for a fresh CLI through that output, and
48.390 ms for a fresh Node process through its fully checked result. These are
three different measurements. Output bytes do not determine startup time.

The reproducible entry points are [benches/compiler.rs](../benches/compiler.rs)
and [run_compiler.py](run_compiler.py), with small
[collection fixtures](fixtures/compiler/collections) and a
[resident Node driver](support/compiler_operations.js). Production Cargo
dependencies and external npm dependencies remain empty.

## Environment and retained evidence

Apple M3 Ultra **virtual machine**, ARM64, 24 visible CPUs, 32 GiB RAM,
macOS 15.7.3; Rust 1.99.0 (`b940084d7eb6a299eb4bfeb8e34901bc051e7ac4`),
LLVM 23.1.2, Cargo 1.99.0 (`5f94df478`), Node v24.18.0, Python 3.14.6.
Release optimization level 3, no custom build environment or Node flags, no
CPU pinning. Benchmarks ran sequentially before the full test suites.

[Environment/options](results/compiler-2026-10-09/collections/environment.json),
[exact source/fixture/binary hashes](results/compiler-2026-10-09/collections/manifest.json),
[all distributions](results/compiler-2026-10-09/collections/summary.json),
[resident samples](results/compiler-2026-10-09/collections/resident.csv),
[allocation samples](results/compiler-2026-10-09/collections/allocations.csv),
[fresh process samples](results/compiler-2026-10-09/collections/fresh-process.csv),
and [operation samples](results/compiler-2026-10-09/collections/operations.json)
retain every measured observation. Each distribution reports nine samples,
median, median absolute deviation (MAD), minimum and maximum. Ranges are not
confidence intervals. No wall-clock or heap threshold is part of unit CI.

The complete local bundle is `/private/tmp/light-cljs-compiler-report-73-final`:
exact source archive, unchanged-baseline archive, five saved executables,
emitted JS and fresh-process drivers, plus the retained compact reports above.
Archives and executable hashes are in the manifests; HEAD alone does not identify
the accumulated uncommitted source. The bundle is a local artifact; reproduction
below creates an equivalent new bundle. Temporary exploratory runs are not this
report's measurements.

## Phase boundaries and validation

There are 48 compiler scenario/phase pairs (432 timing samples and 432 separate
allocation samples), 12 unchanged primitive baseline phase pairs (108 samples),
26 fresh-process pairs (234 samples), and 142 resident operation/size pairs
(1,278 samples). Every resident phase uses two untimed warmup batches and 32
iterations per sample. Assertions and reporting are outside the timed batches;
Rust `black_box` consumes the returned arena/program/string and checksum.
Allocation/drop inside each operation are included.

Parse starts with resident source and builds/drops syntax. Analyze starts with
preparsed syntax, builds/drops a resolved Program, and includes macro work when
present. Emit starts with a preanalyzed Program and builds/drops the entire JS
String, including the runtime. Total starts again from resident source.
Namespace resident phases receive validated headers and dependency order;
`filesystem-*` additionally perform path resolution, file reads, header parsing
and namespace loading. Their timings must not be substituted for in-memory
phase costs. Separately measured phases need not sum exactly to the independent
total.

`macro/host-setup` includes lazy interpreter initialization, evaluation of the
same macro definition and import configuration. Fresh-host analysis/total
include that setup and destruction every iteration. Reused-host phases exclude
it after equal warmups, retain namespace state, and still expand each call.
`macro/expand-resolved` measures `Context::expand_macro_call` on a prepared owned
form, resolved Var and nil environment for `bench.macros/data`, which ignores
`&env`. It excludes parser work, operator resolution, compiler environment
construction and generated-form import/lowering. It is a separately measured
component, not a subtraction estimate or the complete macro analyzer cost.
The macro emits `[1 2 3]`; its compiled output is compared byte-for-byte with
that macro-free source. Single and combined namespace programs both return 6.

Fresh CLI timings start immediately before launching the saved executable and
finish after emitted output has been read and the process exits. Node timings
start before launching Node on a saved driver and finish after the typed result
and diagnostics have been read and Node exits. They include Node startup, file
loading, JS parse/evaluation, the shared existing `tests/support/execution.js`
validator, serialization and pipe/process overhead. They do **not** isolate V8
parse time or predict browser startup. The same validator is included in the
empty and primitive baselines. Both process paths use two separate untimed
warmups; retained job order rotates and reverses across rounds. OS filesystem
and executable caches are uncontrolled, so “fresh process” does not mean cold OS
caches. Compilation and Node execution are timed separately, never as one pipeline.

The existing execution fixture supplies source and typed expected values for the
three primitive cases. CLI output must match API output byte-for-byte. Every
fresh Node result must match the hand-authored full typed result via the shared
outcome encoder; runtime inclusion and helper counts are checked independently.
Resident operation checks inspect branded runtime values, full contents, count,
kind, lookup representatives and retained originals before and after timing.
Map entries and set members are compared after sorting, not in traversal order.
All timed results stay in sample-local slots and are consumed by full validation
after timing; scalar/checksum and slot-store work remains inside timing. No
host-GC, RSS, leak, or native interpreter-GC result is inferred.

## Resident compiler phases

Values are **µs per operation: median / MAD / min–max**.

| Scenario / phase | µs |
| --- | ---: |
| `integer/total` | 0.340 / 0.001 / 0.337–0.348 |
| `collections/parse` | 2.402 / 0.004 / 2.396–2.737 |
| `collections/analyze` | 6.816 / 0.020 / 6.246–7.022 |
| `collections/emit` | 1.484 / 0.006 / 1.452–1.503 |
| `collections/total` | 10.708 / 0.038 / 10.479–10.927 |
| `expanded-macro/total` | 1.438 / 0.004 / 1.428–1.445 |
| `single/total-resident` | 2.969 / 0.069 / 2.900–3.430 |
| `combined/total-resident` | 3.378 / 0.009 / 3.357–3.402 |
| `macro/host-setup` | 165.518 / 1.448 / 163.397–169.607 |
| `macro/expand-resolved` | 3.492 / 0.026 / 3.466–3.685 |
| `macro/analyze-reused-host` | 15.324 / 0.315 / 14.335–16.732 |
| `macro/analyze-fresh-host` | 185.389 / 1.829 / 183.560–204.667 |
| `macro/total-reused-host` | 15.857 / 0.065 / 15.553–16.363 |
| `macro/total-fresh-host` | 187.395 / 2.483 / 184.911–201.617 |
| `bench.single/filesystem-fresh-host` | 30.208 / 0.488 / 29.246–30.865 |
| `bench.main/filesystem-fresh-host` | 47.811 / 0.372 / 42.965–53.448 |
| `bench.macro/filesystem-reused-host` | 47.461 / 1.755 / 43.583–51.966 |
| `bench.macro/filesystem-fresh-host` | 265.154 / 4.823 / 245.672–269.977 |
| `bench.macro-main/filesystem-reused-host` | 63.215 / 0.284 / 62.908–64.340 |
| `bench.macro-main/filesystem-fresh-host` | 266.461 / 1.999 / 264.376–274.469 |

A reused host materially reduces this tiny macro's total cost. It does not make
macro expansion free, and filesystem work remains visible. The macro-free
namespace pair has identical emitted bytes despite different source-file counts.

Allocation counts use a **separate** `count-allocations` build; its timing fields
are zero by design. These are requested Rust layouts, including realloc request
sizes, not live memory, allocator usable sizes, RSS, or JavaScript allocation.
Selected median counts per complete compilation are:

| Scenario | Alloc calls | Realloc calls | Requested bytes |
| --- | ---: | ---: | ---: |
| `integer/total` | 5 | 1 | 842 |
| `collections/total` | 151 | 24 | 95,678 |
| `macro/total-reused-host` | 314 | 47 | 91,740 |
| `macro/total-fresh-host` | 3,108 | 807 | 310,499 |

All four rows have zero MAD in those fields. The reused macro host retains
capacity/history: its requested-byte range is 91,740–153,180 per operation and
reallocation range 47–47.0625, averaged over 32 calls. Other selected rows have
identical min/median/max counts. Reporting only the median would hide this
occasional retained-host capacity growth; full distributions remain available.

## Output size and fresh-process results

Sizes are raw emitted UTF-8, including the trailing newline, and standard-library
gzip with timestamp zero. They exclude the test driver. Gzip is optional transport
accounting, not a minifier or startup measurement. Every output has one truth
helper; these fixtures need no host-global helper. The separate operation-export
fixture has one runtime, one truth helper and one host helper; its
[size record](results/compiler-2026-10-09/collections/operation-output-size.json)
is retained too.

| Program | Raw bytes | Gzip bytes | Runtime copies |
| --- | ---: | ---: | ---: |
| `integer` | 104 | 106 | 0 |
| `nested-closure` | 313 | 168 | 0 |
| `js-nested-method` | 142 | 133 | 0 |
| `empty` | 106 | 103 | 0 |
| `collections` | 24,944 | 5,642 | 1 |
| `expanded-macro` | 24,235 | 5,506 | 1 |
| `bench.single` | 24,303 | 5,535 | 1 |
| `bench.main` | 24,303 | 5,535 | 1 |
| `bench.macro` | 24,280 | 5,522 | 1 |
| `bench.macro-main` | 24,303 | 5,535 | 1 |

All three primitive outputs are byte-identical to the unchanged baseline. The
24,090-byte runtime is embedded once in each collection unit, including both
combined namespaces. No runtime appears once per literal, function or namespace.
The fixed runtime costs output bytes even for `[1 2 3]`; compression reduces
transport bytes but does not erase evaluation or construction work.

Fresh-process values are **ms: median / MAD / min–max**.

| Current program | CLI through output | Node through checked result |
| --- | ---: | ---: |
| `empty` | 2.273 / 0.123 / 2.072–2.905 | 47.531 / 1.842 / 45.689–54.617 |
| `integer` | 2.185 / 0.116 / 2.069–2.427 | 48.240 / 0.836 / 46.214–55.009 |
| `collections` | 2.311 / 0.061 / 2.176–3.222 | 48.390 / 0.854 / 47.082–57.663 |
| `bench.single` | 2.408 / 0.102 / 2.284–2.647 | 48.305 / 1.002 / 47.123–53.157 |
| `bench.main` | 2.502 / 0.086 / 2.281–2.770 | 48.682 / 1.107 / 47.212–50.849 |
| `bench.macro` | 2.871 / 0.138 / 2.705–3.242 | 48.431 / 0.659 / 47.433–53.480 |
| `bench.macro-main` | 2.890 / 0.111 / 2.603–3.557 | 48.273 / 1.049 / 47.165–53.715 |

The Node distributions overlap substantially. This experiment does not resolve a
precise runtime-only startup penalty: process and harness overhead dominate these
small units. In particular, 24 KB of extra source is not evidence of a proportional
startup slowdown.

## Immutable operation costs

Sizes are 8, 64, 512 and 4,096. Inputs, structural lookup keys and construction
argument arrays are prepared before timing. Vector construction copies a host
argument array through the emitted first-class constructor; vector assoc/conj
return new versions while originals remain reachable. Lists measure prepend and
complete first/next traversal. Map/set keys are `[i]` vectors queried with equal
`(i)` lists, including misses. Collision keys are distinct 24-character products
of `Aa`/`BB`; the driver asserts all hashes collide before timing. Updates retain
the first key/member representative. Both normal and colliding scenarios cover
construction, successful lookup, misses, assoc/dissoc/disj/conj and callable data.
Saved core calls, compiled callable keyword/vector/map/set wrappers and nested
structural-key map update/lookup are also included.

Most operations use 32 repetitions per sample. Bulk map/set construction uses
`max(1, min(iterations, floor(16384 / size²)))`: **32, 4, 1, 1** repetitions at the
four sizes, with the same two warmup batches. This keeps quadratic workloads
bounded while retaining nine raw observations. Nested-map scenarios keep a
one-entry outer/inner shape at every size and vary the stored scalar; their size
labels are not claims of nested-map scaling. Sizes run in ascending order in one
Node process. V8 tiering, warm caches and timer/checksum overhead affect small
operations; decreasing `nth` measurements do not mean large vectors improve its
asymptotic cost. No forced GC or heap measurement is performed. Untimed full-value validation
may allocate inspection snapshots; subsequent automatic V8 collection can appear
in a timed sample. These are host-runtime observations, not isolated allocator
costs or leak guarantees.

Selected median **µs per operation** (full MAD/ranges for all 142 pairs are in the
linked summary):

| Operation | 8 | 64 | 512 | 4,096 |
| --- | ---: | ---: | ---: | ---: |
| `vector/construct` | 0.227 | 0.233 | 0.547 | 3.684 |
| `vector/nth` | 0.135 | 0.065 | 0.039 | 0.023 |
| `vector/assoc-retained` | 0.648 | 2.279 | 11.908 | 99.444 |
| `vector/conj-retained` | 0.233 | 0.257 | 0.473 | 4.289 |
| `list/prepend-retained` | 0.372 | 0.283 | 0.224 | 0.333 |
| `list/traverse` | 0.490 | 1.898 | 12.796 | 163.440 |
| `map/structural/construct` | 2.432 | 32.864 | 754.167 | 40,803.084 |
| `map/structural/lookup-last` | 0.896 | 2.301 | 11.060 | 88.064 |
| `map/collision/construct` | 1.854 | 71.823 | 3,924.417 | 261,723.041 |
| `map/collision/lookup-last` | 0.544 | 3.599 | 24.802 | 200.594 |
| `set/structural/construct` | 3.001 | 37.177 | 744.959 | 39,416.625 |
| `set/structural/lookup-last` | 0.982 | 2.181 | 10.655 | 85.819 |
| `set/collision/construct` | 2.172 | 72.906 | 4,027.084 | 263,248.500 |

The large-size data agrees with the deliberate representation costs: copied
vector updates and linear map/set lookup; naive distinct-entry construction is
quadratic before nested-key hash/equality work. At 4,096 entries, structural-key
map construction takes about 40.8 ms and all-colliding map construction 261.7 ms.
The corresponding set costs are 39.4 ms and 263.2 ms. These negative findings are
retained, not grounds for an unrelated trie/optimizer rewrite. `assoc` and `conj`
have substantially different constants despite both copying vector storage;
this report does not claim identical implementation cost from identical Big-O.

The semantically equivalent numeric fold uses a prebuilt 0..999 immutable vector
for `(reduce + 0 ...)` and a prebuilt JS array for a compiled `.reduce` with a
compiled callback invoking the host's numeric addition function. Both return
**499500**, leave full inputs intact, and exclude input construction. It compares
the delivered paths, not identical callback mechanics or collection semantics.
Structural map/set lookup is never compared to JS object/Map identity lookup.

| Numeric fold, 1,000 elements | µs: median / MAD / min–max |
| --- | ---: |
| `fold/core-reduce-add/1000` | 16.070 / 0.380 / 15.435–17.525 |
| `fold/compiled-js-interop/1000` | 4.622 / 0.006 / 4.609–4.986 |

The core reduction is about 3.5× slower for this finite numeric workload. This
comparison does not support a universal speed claim for either collection path.

## Unchanged primitive baseline

The runner archives commit `fe08ac83dc067ca85344663f2d0f2a96e613827e`, copies
only the measurement harness/fixtures into the disposable source archive, and
builds separate CLI and benchmark executables. It neither checks out nor modifies
a Git worktree. The original rows for `integer`, `nested-closure` and
`js-nested-method` must match the current source and expected values exactly.
[Baseline identity](results/compiler-2026-10-09/collections/baseline-identity.json)
and [all baseline phase samples](results/compiler-2026-10-09/collections/primitive-baseline.csv)
are retained. Builds are excluded; the resident baseline runs sequentially after
the current sample batches, whereas fresh-process jobs alternate by rotated order.
This compares complete snapshots, including accumulated native changes, and
cannot isolate the causal effect of the collection compiler changes.

| Unchanged fixture total | Baseline µs | Current µs | Observed change |
| --- | ---: | ---: | ---: |
| `integer` | 0.309 | 0.340 | +10.1% |
| `nested-closure` | 3.055 | 3.322 | +8.7% |
| `js-nested-method` | 1.148 | 1.122 | -2.3% |

The two observed increases and one decrease are preserved. Sub-microsecond phase
measurements and single sequential A/B runs are sensitive to scheduling, thermal
state and code layout; this is not proof of an isolated regression or speedup.
Output bytes and primitive results remain unchanged. The full comparison includes
parse, analyze, emit and total, as well as separately retained fresh CLI/Node
samples; selecting just the improved row would misrepresent the evidence.

## Reproduction and correctness checks

From the repository root (Python standard library only; Rust/Cargo and Node must
be installed), use a **new** output directory outside `target`:

```sh
cargo bench --locked --bench compiler -- --samples 9 --iterations 32
cargo bench --locked --bench compiler --features count-allocations -- \
  --allocations --samples 9 --iterations 32
python3 benchmarks/run_compiler.py --samples 9 --iterations 32 \
  --baseline-ref fe08ac83dc067ca85344663f2d0f2a96e613827e \
  --output /tmp/light-cljs-compiler-report-new
cargo test --locked
cargo test --locked --release
LIGHT_CLJS_REFERENCE=release cargo test --locked --test execution -- --ignored --nocapture
cargo clippy --locked --all-targets -- -D warnings
cargo clippy --locked --all-targets --features count-allocations -- -D warnings
cargo fmt --check
```

The runner also accepts `--samples` and `--iterations` directly and refuses fewer
than nine samples, zero iterations, overwritten output directories, changed
primitive fixtures or mismatched results. Omit `--baseline-ref` for a standalone
current-snapshot report. The benchmark's `--primitive-only` mode is used by the
saved baseline. Counted and ordinary timing builds reject mismatched allocation
flags. No optimizing compiler, tree shaking, minifier, external dependency or
runtime implementation was added by this measurement work.

Completed verification for the retained snapshot: **588 passed / 12 opt-in ignored**
in each of debug and release; all **8** pinned Maven ClojureScript 1.12.134
reference tests passed (517 outcomes, 48 generated programs, 130 analyzer
boundaries, typed collection namespace/macro program and primitive namespace
checks). Both ordinary and allocation-feature all-target strict Clippy checks,
formatting, diff whitespace checks and Node driver syntax checks passed. Four
invalid sample/allocation-flag cases were explicitly rejected. See the
[verification record and retained logs](results/compiler-2026-10-09/collections/verification.json).
The source hashes still match the measured inputs; documentation was added after
timing. No production implementation was changed by this benchmark pellet.

## Language integration, October 10, 2026

The [integrated source program](../tests/fixtures/namespaces/integrated) combines
`.cljs`, shared `.cljc`, and a native macro namespace. The complete value, ordered
effects and stdout are validated for every sample, including its 100,000-step
recurrence. This measurement uses the final language-completion snapshot; the
collection and primitive baseline measurements above retain their original inputs.

| Compiled program | Raw bytes | gzip bytes | Runtime copies |
| --- | ---: | ---: | ---: |
| Literal `42` | 88 | 93 | 0 |
| Core call `(+ 1 2)` | 174,302 | 40,661 | 1 |
| Integrated namespace program | 183,146 | 42,666 | 1 |

Primitive-only output remains small. Calling a core function currently loads the
entire 174,172-byte runtime (40,628 bytes gzip), even for this two-number addition.
The expanded runtime therefore carries a substantial fixed output cost. Gzip can
make the smallest program larger. These rows are actual emitted programs, not
minified estimates or a size comparison against differently wrapped old fixtures.

Nine fresh Node v24.18.0 processes on macOS 15.7.3 ARM64 time `vm.Script` parsing,
namespace initialization, and the integrated execution separately. Source reads,
process launch, and result encoding are excluded. OS caches and scheduling are
uncontrolled, and no compiler builds or other verification jobs ran concurrently.

| Integrated phase | Median ms | Min–max ms |
| --- | ---: | ---: |
| Parse generated JavaScript | 2.479 | 2.439–2.767 |
| Initialize namespace program | 1.960 | 1.594–2.490 |
| Execute program | 5.740 | 5.536–7.463 |
| Parse through execution | 10.090 | 9.919–12.719 |

Phase medians need not sum to the median total. These Node observations do not
establish browser startup performance, a regression relative to the old snapshot,
or a performance budget. The [raw samples, hashes and full expected result](results/compiler-2026-10-10/language-completion/output-startup.json)
and [emitted integrated program](results/compiler-2026-10-10/language-completion/integrated.js)
are retained. Reproduce with:

```sh
python3 -B benchmarks/measure_compiler_language.py \
  --output /tmp/light-cljs-language-measurement
```

## Evaluator migration gate — October 10, 2026

The native-disabled execution gate was run at this point; its verification
notes were removed from the tree and remain in git history.
Historical native macro/collector timings above remain historical; production
macros and evaluation consumers execute compiler-emitted JavaScript in QuickJS.
The native bench targets and runner scripts were physically removed under light-cljs-115.
The initial gate/test migration left the canonical runtime unchanged. The
resumed verification also repairs the retained applied-constructor lazy-tail
contract; its refreshed measurements appear below.

Executed the existing application preparation benchmark with native entrypoints
fenced, Node v24.18.0, Rust 1.99.0, and nine samples per scenario/mode/phase:

```sh
LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 cargo bench --locked --bench compiler -- --application-only --samples 9 --iterations 1 --dump /tmp/light-cljs-114-performance
```

The run exited 0 with **270 measured samples**. The
[raw CSV](results/evaluator-migration-2026-10-10/application.csv) retains every
sample. Preparation and prepared emission start from previously analyzed IR;
Node startup includes process launch, parsing, execution and result comparison
against unoptimized output. Every invocation, including warmups, checked its
complete serialized result. These measurements do not measure macro compilation
or the persistent QuickJS session path.

| Scenario | Output bytes, off / on | Prepared emission median µs, off / on | Node median ms, off / on |
| --- | ---: | ---: | ---: |
| integer | 88 / 88 | 0.292 / 0.292 | 123.467 / 60.786 |
| nested-closure | 297 / 297 | 1.334 / 1.417 | 56.404 / 56.714 |
| js-nested-method | 126 / 126 | 0.625 / 0.666 | 57.650 / 57.175 |
| collections | 233,326 / 233,326 | 10.291 / 10.250 | 63.739 / 63.384 |
| macro-helpers | 237,669 / 233,493 | 49.541 / 16.959 | 62.565 / 61.558 |

Macro-only helper elimination still removes **4,176 bytes**, and all off/on
values agree. Concurrent debug Cargo builds contended with this local run,
especially its initial integer/off samples; these noisy timings establish no
speedup, reliable performance regression or comparison to historical snapshot
startup. No allocation, cross-platform, old native-GC or full legacy benchmark
run is claimed. The initial three-sample invocation was rejected by the existing
benchmark's minimum-nine-samples contract; the corrected invocation above is the
completed measurement.

### Final resumed application measurement

After all default-stack Cargo and native-disabled/reference gates passed, the
existing benchmark was rerun against the constructor repair, with the same tools
and nine samples per group. No concurrent compiler build was running.

```sh
LIGHT_CLJS_DISABLE_NATIVE_EVAL=1 cargo bench --locked --bench compiler -- --application-only --samples 9 --iterations 1 --dump /tmp/light-cljs-114-resume-performance
```

Exit 0; **270 samples**, every complete off/on result checked, including warmups.
[Final raw CSV](results/evaluator-migration-2026-10-10/final-application.csv)
retains all samples.

| Scenario | Output bytes, off / on | Prepared emission median µs, off / on | Node median ms, off / on |
| --- | ---: | ---: | ---: |
| integer | 88 / 88 | 0.292 / 0.333 | 56.678 / 53.851 |
| nested-closure | 297 / 297 | 1.417 / 1.208 | 52.916 / 52.982 |
| js-nested-method | 126 / 126 | 0.625 / 0.792 | 53.701 / 53.058 |
| collections | 234,491 / 234,491 | 10.833 / 9.542 | 57.633 / 58.482 |
| macro-helpers | 238,834 / 234,658 | 36.000 / 29.250 | 57.045 / 58.316 |

The canonical runtime gains **1,165 emitted bytes** for the bounded constructor
ABIs. Primitive output sizes remain identical, and macro-only helper elimination
still removes **4,176 bytes** with equal complete results. Node startup medians
span 52.916–58.482 ms. The earlier run had concurrent-build contention; it is
unsuitable for claiming a reliable speedup or regression from these samples.
These are application preparation/emission and fresh Node measurements, not
macro compilation, persistent QuickJS throughput, allocation or native GC
measurements.

## Optimization-level size ratchet — October 12, 2026

Baseline for the [advanced optimization](../docs/advanced-optimization.md)
work (light-cljs-201). `--optimize none|prepare|advanced` selects the
application level; `advanced` does not change output yet. The ratchet in
[tests/compiler/size_ratchet.rs](../tests/compiler/size_ratchet.rs) pins these
sizes with ceilings rounded up to the next 1,000 bytes; later pellets lower
them and add rows here. Raw bytes are the emitted expression
(`emit_js()`, equal to `--compile --optimize LEVEL --text` output); gzip is
`gzip -9`.

| Fixture | Source | Raw bytes, none / prepare / advanced | Gzip bytes | Ceiling |
| --- | --- | ---: | ---: | ---: |
| console-log-sum | `(js/console.log (+ 1 2))` | 350,927 / 350,927 / 350,927 | 83,881 | 351,000 |
| println-hi | `(enable-console-print!) (println "hi")` | 350,814 / 350,814 / 350,814 | 83,823 | 351,000 |
| vector-printed | `(enable-console-print!) (println [1 2 3])` | 350,839 / 350,839 / 350,839 | 83,831 | 351,000 |
| map-printed | `(enable-console-print!) (println {})` | 350,829 / 350,829 / 350,829 | 83,824 | 351,000 |
| seq-pipeline | upstream `trivial/core6.cljs` expression | 351,158 / 351,158 / 351,158 | 83,941 | 352,000 |
| add | `(+ 1 2)` | 350,767 / 350,767 / 350,767 | 83,802 | 351,000 |
| rest-fn | `(fn [& xs] xs)` | 351,111 / 351,111 / 351,111 | 83,934 | 352,000 |
| keyword-case | `case` on keywords in a `defn` | 351,399 / 351,399 / 351,399 | 83,958 | 352,000 |
| record-protocol | `defrecord` with a protocol call | 351,720 / 351,720 / 351,720 | 84,122 | 352,000 |

Every fixture embeds the whole runtime, so the sizes differ by a few hundred
bytes of program code. The three levels are equal because none of these
programs has an unused definition or a specializable call; gzip is the same at
every level.

## Runtime units — October 12, 2026

light-cljs-202 split `src/js_runtime.js` into units with `//@unit` headers;
[`src/runtime_manifest.rs`](../src/runtime_manifest.rs) documents the format
and validates every header against a lexical scope analysis of its body. The
headers are build-time annotations: `build.rs` strips them, so every emitted
program still embeds the same **350,414-byte** runtime, byte for byte, and the
size ratchet above is unchanged. The
annotated source is 408,980 bytes, of which 58,566 bytes are 936 header lines.

| Partition | Count |
| --- | ---: |
| Units | 691 |
| Declaration units (one top-level declaration statement each) | 658 |
| of which also carry effects on their own bindings | 6 |
| Effect units that belong to other units' bindings | 33 |
| Top-level bindings | 730 |
| Declared `requires` edges | 3,351 |

Unit bodies hold 349,584 bytes; the prologue and the export section hold the
other 830. Effect units are the 24 `variadicCore` registrations (one per
operation, each kept with its operation), the builtin type installs and
`EMPTY` constants (`builtinTypes.*`), the reader and host/namespace
registrations on `core`, `Object.seal(core)`, protocol defaults, the `Var`
prototype and the `cljs.core` namespace. Bytes include each unit's comments
and blank lines.

| Largest units | Bytes | Requires | Reach units | Reach bytes |
| --- | ---: | ---: | ---: | ---: |
| `core.reader` (EDN/code reader) | 35,060 | 18 | 665 | 334,504 |
| `builtinTypes.prototypes` | 13,021 | 65 | 665 | 334,504 |
| `core` (the hub literal) | 12,573 | 403 | 665 | 334,504 |
| `compilerFormBridge` | 11,650 | 38 | 665 | 334,504 |
| `readerCoreNames` | 9,366 | 0 | 1 | 9,366 |
| `builtinCoreImplementation` | 6,880 | 85 | 50 | 12,623 |
| `reduce` | 4,905 | 19 | 665 | 334,504 |
| `printWriterImpl` | 4,061 | 28 | 665 | 334,504 |
| `installMethods` | 3,967 | 12 | 665 | 334,504 |
| `withMeta` | 3,719 | 42 | 665 | 334,504 |

"Reach" is what [`RuntimeManifest::reachable`](../src/runtime_manifest.rs)
keeps for that unit alone: the closure over `requires`, plus every effect unit
that targets a kept binding. Today any value constructor keeps nearly
everything: `keyword`, `symbol`, `vectorFromArray`, `mapFromArray`,
`setFromArray`, `hash`, `equal`, `str` and `print` each reach the same 665
units (334,504 bytes, 95.7%). Leaf units such as `truth` (59 bytes) and
`invalidArity` (73 bytes) reach only themselves; 200 units reach fewer than 50
units and 468 reach at least 400. The shortest paths name the edges that the
next Phase 1 steps cut:

- `keyword → makeKeyword → builtinValue → builtinType`, which references every
  builtin constructor, so every constructor's prototype comes along;
- `vectorFromArray → makeVector → vectorPrototype → print`: collection
  `toString` reaches the printer (step 6);
- `print → core` and `ChunkedCons → core`: the `core` hub requires 403
  bindings, and its effect units (`core.reader`, `core.hostAndNamespaces`)
  follow it (step 3);
- `builtinType → builtinTypes`, whose install effects require 65 more (step 4).

The [full table](results/runtime-units-2026-10-12/units.tsv) lists every
unit's bytes, requires count, effect targets and reach. Reproduce it with:

```sh
cargo test --test compiler runtime_manifest::print_runtime_unit_sizes -- --ignored --nocapture
```

## Advanced runtime slicing — October 12, 2026

light-cljs-203 makes `--optimize advanced` embed only the runtime units
reachable from the runtime names the emitted code reads (Phase 1 step 2 of the
[design](../docs/advanced-optimization.md)). `none` and `prepare` output is
unchanged. Raw bytes are `--compile --optimize LEVEL --text` output; gzip is
`gzip -9` with the gzip that ships with macOS 15, which compresses these
programs a few bytes differently from the run recorded in the ratchet section
above.

| Fixture | Prepare raw / gzip | Advanced raw / gzip | Saved raw | Units kept | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 350,927 / 83,889 | 335,242 / 80,088 | 15,685 | 665 | 336,000 |
| println-hi | 350,814 / 83,831 | 335,129 / 80,030 | 15,685 | 665 | 336,000 |
| vector-printed | 350,839 / 83,839 | 335,171 / 80,038 | 15,668 | 665 | 336,000 |
| map-printed | 350,829 / 83,832 | 335,158 / 80,033 | 15,671 | 665 | 336,000 |
| seq-pipeline | 351,158 / 83,949 | 335,473 / 80,151 | 15,685 | 665 | 336,000 |
| add | 350,767 / 83,810 | 335,082 / 80,008 | 15,685 | 665 | 336,000 |
| rest-fn | 351,111 / 83,942 | 335,445 / 80,157 | 15,666 | 665 | 336,000 |
| keyword-case | 351,399 / 83,966 | 335,723 / 80,165 | 15,676 | 665 | 336,000 |
| record-protocol | 351,720 / 84,130 | 344,607 / 82,473 | 7,113 | 672 | 345,000 |

This is the before number for dismantling the `core` hub (light-cljs-204).
Every program that reads the runtime reads some `rt.core.*` name, if only in
the namespace registration emitted with the runtime, and `core` keeps 665 of
the 691 units (334,504 of 349,584 unit bytes). The 26 units it does not reach,
15,080 bytes, are what a slice can drop today: the declaration demand and
patch machinery (`setDeclarationDemand`, `declarationReceiver`,
`setDeclaration`, `declarationTransaction` and the three patch helpers),
`setMacroexpandServices`, `compilerExceptionMessage`, the protocol, type,
record and reify constructors (`protocol`, `protocolMethod`,
`protocolPredicate`, `extendType`, `extendInstance`, `makeReify`, `reify`,
`reifySites`, `makeType`, `recordMethods`, `recordCopy`, `recordFromMap`),
`definitionVar`, and the literal constructors `mapFromArrays`,
`constantMapFromArray`, `constantSetFromArray` and `literalSetFromArray`. The
`defrecord` fixture reads `makeType`, `protocol`, `protocolMethod` and
`recordFromMap`, which bring 7 of them back. The rest of the saving is the
export section, which lists only the exports the program reads.

`--size-report` prints which units an output keeps and why; the report for
`console-log-sum` at `advanced` is checked in as
[results/retention-2026-10-12/console-log-sum.txt](results/retention-2026-10-12/console-log-sum.txt).
Every unit there is attributed to `rt.core.add`, the first runtime read.
Reproduce it with:

```sh
cargo run --quiet -- --compile --optimize advanced --size-report \
  --text '(js/console.log (+ 1 2))' 2>&1 >/dev/null
```

## Generated dead-code baseline — October 12, 2026

Baseline of what advanced output keeps of known-dead code (light-cljs-217),
from the twelve fixed seeds of
[tests/compiler/generated_optimization.rs](../tests/compiler/generated_optimization.rs)
(`--no-capture` prints the per-seed tables). Each step adds one tag's garbage
to the program; bytes are the growth of the normalized advanced output
(identifier digits dropped, so renumbered compiler names do not count).

| Garbage tag | Items | Code survived | Bytes added per item | Class |
| --- | ---: | ---: | ---: | --- |
| unreferenced-namespace | 18 | 0 | 223-289 | residue |
| unused-literal-def | 23 | 0 | 24-25 | residue |
| unused-pure-def | 19 | 19 | 142-279 | survives |
| unused-defn | 20 | 0 | 24-25 | residue |
| unused-keyword | 20 | 0 | 24-25 | residue |
| uninstantiated-type | 19 | 19 | 513-6,411 | survives |
| uncalled-protocol-method | 16 | 16 | 193-461 | survives |

Measured with advanced runtime slicing (light-cljs-203): in seeds 0, 17 and
255 a surviving type keeps runtime units the program does not otherwise reach
(about 5-6 KB); before slicing every type cost 509-613 bytes. The residue of a removed definition is its
`null;` statement and its name in the namespace's `registerReaderNamespace`
bindings; an unreferenced namespace also keeps its registration and
`currentNs` switches. Prepare removed 8-15
definitions per program with all garbage and specialized 1-2 calls in six of
the twelve programs, with or without garbage.

## Core operation table — October 13, 2026

light-cljs-209 annotated all 688 `CoreFnId` variants in
[`src/core_operations.tsv`](../src/core_operations.tsv) with types, purity,
the argument code they may run and their runtime units
([`src/core_operations.rs`](../src/core_operations.rs)). It is compile-time
data only: emitted output and the size ratchet are unchanged.

| Rows | Count |
| --- | ---: |
| With call signatures | 417 |
| of which `pure` / `allocates` / `throws` / `effect` | 182 / 108 / 42 / 85 |
| Without (types, protocols, dynamic vars, protocol method dispatchers, compiler and reader services) | 271 |
| Calls: none / `dispatch` / `invoke` (all rows) | 284 / 94 / 310 |

The rows name 364 distinct units. 261 callable operations are defined by a
unit of their own binding (`add` for `+`, `keywordCore` for `keyword`); the
others live in shared units: `numeric` (73), `predicates` (46), the `core` hub
literal itself (126 rows, 36 of them callable, such as `second`, `some`,
`identical?` and `instance?`), `core.reader` (21) and
`core.hostAndNamespaces` (3). Dismantling the hub (Phase 1 step 3) has to
give those a binding before a direct reference can replace `rt.core.X`.
Today 287 of the 364 units (563 of the 688 rows) still reach the whole
334,504-byte unit closure; 77 units (125 rows) reach under 20 KB: protocol
markers and leaf operations such as `subs`, `char`, `mod`, `parse-long`,
`name`, `satisfies?` and `deref`. Reproduce the per-unit table with:

```sh
cargo test --test compiler core_operations::print_core_operation_units -- --ignored --nocapture
```

## Startup and steady state by optimization level — October 13, 2026

light-cljs-216 adds a `--levels` mode to [benches/compiler.rs](../benches/compiler.rs)
([support/levels.rs](../benches/support/levels.rs)). It times every size-ratchet
fixture ([tests/support/ratchet_fixtures.rs](../tests/support/ratchet_fixtures.rs))
at every `--optimize` level, from a fresh engine, in Node and in QuickJS. It also
times steady-state calls of a sequence pipeline and a record/protocol workload,
and can record upstream ClojureScript's `:advanced` sizes for the same programs.
To measure before and after a change, run it on both commits, ideally on a quiet
machine:

```sh
cargo bench --locked --bench compiler -- --levels --output benchmarks/results/levels-$(date +%F)
```

`--upstream research/clojurescript` adds the upstream column. It needs Java and
the Clojure CLI, and adds about a minute. `--samples N` (default 15, at least 9)
and `--warmups N` (default 2) set the rounds. With the defaults the run takes
about three minutes. It writes `samples.csv` (every sample, with the one-minute
load average of its round) and `summary.md` (every table below, with environment
and load), and it refuses a directory that already holds a run. The application
phases of the default mode (`--application-only`) now also run at all three
levels: their scenarios are `NAME-none`, `NAME-prepare` and `NAME-advanced`
instead of `NAME-off` and `NAME-on`.

Method:

- **Programs.** `prepare_application(.., level).emit_js()` of each fixture is
  emitted and written to a temporary directory before timing.
- **Node.** Each sample is a fresh `node` process running
  [support/compiler_levels.js](support/compiler_levels.js) with no
  `NODE_OPTIONS` and no compile cache. *Parse* is `new vm.Script(source)` and
  *execute* is its first `runInThisContext()`. Node's lazy stdout and console
  setup (about 5 ms) is done before timing, so it does not land on whichever
  program prints first. *Process* is spawn to exit, measured by Rust.
- **QuickJS.** Each sample is a fresh `light_cljs::evaluator::Evaluator`, the
  engine that `EvalSession` embeds, with a `console.log` shim. The timing covers
  one evaluation of the program; QuickJS parses and runs in one step, so there
  is no split. The session loads its own copy of the shared runtime before
  timing starts, and that load is not counted.
- **Steady state.** Each workload evaluates to `(fn [n] ...)`. *seq-pipeline*
  sums the ratchet's pipeline over `(range n)` with n = 200. *record-protocol*
  builds the ratchet's `Rect` and calls `area` on it n = 2,000 times in a
  `loop`. Each sample is a fresh process or session that calls the function 10
  times untimed and then 20 times in one timed batch. QuickJS calls go through
  `Evaluator::call`, which costs a few µs, under 0.1% of a call.
- **Noise.** The machine is shared, so every round runs each fixture at each
  level before the next round starts, rotating the level order. A burst of
  outside load therefore lands on all levels alike. Two untimed warm-up rounds
  come first. The tables give medians, and `summary.md` also gives minimums.
- **Validation.** Every sample is checked, including warm-ups. Its stdout must be
  the fixture's expected `printed` text, and a steady-state result must equal
  the value Rust computes for it.
- **Upstream.** Each fixture becomes namespace `bench.NAME`, with its last form
  wrapped to log its value the way the Node driver does. It is compiled with
  `cljs.build.api/build` from the pinned checkout (used as a `:local/root`
  dependency) at `:optimizations :advanced`
  ([support/upstream_sizes.clj](support/upstream_sizes.clj)), and again with
  `:lite-mode true :elide-to-string true` as upstream's lite-mode ratchets do.
  Each output must print the fixture's expected text in Node. Without Java,
  the Clojure CLI or the checkout, the column is skipped with the reason.

Baseline: master `95a5e92` (light-cljs-203's runtime slicing at `advanced`).
Node v24.18.0, macOS 15.7.3 ARM64 VM, 24 CPUs; 2 warm-up and 15 timed rounds.
The machine was shared but fairly quiet: the one-minute load average was 2.6–3.2
during the run. [summary.md](results/levels-2026-10-13/summary.md) has every
measure with its minimum, and
[samples.csv](results/levels-2026-10-13/samples.csv) has all 2,205 samples.

Parse plus first execution, median ms:

| Fixture | Bytes, none / advanced | Node, none / prepare / advanced | QuickJS, none / prepare / advanced |
| --- | ---: | ---: | ---: |
| console-log-sum | 350,927 / 335,242 | 7.17 / 7.18 / 7.01 | 16.95 / 17.34 / 16.28 |
| println-hi | 350,814 / 335,129 | 7.71 / 7.61 / 7.44 | 17.24 / 17.41 / 16.45 |
| vector-printed | 350,839 / 335,171 | 7.96 / 7.82 / 7.58 | 17.63 / 17.20 / 16.68 |
| map-printed | 350,829 / 335,158 | 7.81 / 7.73 / 7.61 | 17.25 / 17.54 / 16.55 |
| seq-pipeline | 351,158 / 335,473 | 9.03 / 9.16 / 8.90 | 18.31 / 18.33 / 17.56 |
| add | 350,767 / 335,082 | 6.75 / 6.83 / 6.47 | 17.11 / 17.30 / 16.61 |
| rest-fn | 351,111 / 335,445 | 6.78 / 6.75 / 6.49 | 17.36 / 17.42 / 16.50 |
| keyword-case | 351,399 / 335,723 | 6.84 / 6.83 / 6.68 | 17.21 / 17.16 / 16.62 |
| record-protocol | 351,720 / 344,607 | 7.22 / 7.25 / 7.09 | 17.44 / 17.46 / 17.12 |

Steady state, median ms per call:

| Workload | n | Node, none / prepare / advanced | QuickJS, none / prepare / advanced |
| --- | ---: | ---: | ---: |
| seq-pipeline | 200 | 5.25 / 5.17 / 5.13 | 29.64 / 29.54 / 29.71 |
| record-protocol | 2,000 | 9.08 / 9.14 / 9.20 | 20.07 / 20.11 / 20.22 |

Upstream ClojureScript (checkout `a5e94c98`), bytes of `main.js`:

| Fixture | Upstream `:advanced` | Upstream lite mode | light-cljs `advanced` | light-cljs / upstream `:advanced` |
| --- | ---: | ---: | ---: | ---: |
| console-log-sum | 2,584 | 2,135 | 335,242 | 130× |
| println-hi | 88,554 | 37,284 | 335,129 | 3.8× |
| vector-printed | 88,582 | 37,300 | 335,171 | 3.8× |
| map-printed | 88,552 | 56,119 | 335,158 | 3.8× |
| seq-pipeline | 105,760 | 32,508 | 335,473 | 3.2× |
| add | 2,529 | 2,080 | 335,082 | 132× |
| rest-fn | 88,289 | 10,726 | 335,445 | 3.8× |
| keyword-case | 5,783 | 4,756 | 335,723 | 58× |
| record-protocol | 2,623 | 2,174 | 344,607 | 131× |

What the baseline shows:

- **Noise floor.** `none` and `prepare` emit identical bytes for every fixture,
  and their medians differ by at most 1.8% in Node and 2.5% in QuickJS. A single
  run cannot resolve a startup change smaller than that.
- **Startup cost.** The embedded runtime dominates startup. Node parses each
  ~350 KB program in about 4.1 ms. Running it, which is mostly runtime
  initialization, takes 2.6 ms for `(+ 1 2)` and up to 5.0 ms for the
  pipeline. QuickJS does both in about 17 ms. A whole Node process takes about
  57 ms, so level differences are invisible at that scale.
- **Advanced slicing.** At `advanced`, 203's slice removes 4.5% of the bytes. On
  all eight fixtures it shrinks that much, it already cuts Node parse time by
  2.5–5.5% (0.1–0.2 ms) and QuickJS parse plus execution by 3–5.5% (0.5–1.0
  ms). The smaller cuts are close to the noise floor. `record-protocol` keeps
  more of the runtime (2% fewer bytes), so it saves about half as much. Parse
  cost so far tracks bytes.
- **Steady state.** No level changes program code yet, so steady state is the
  same at every level. The pipeline costs about 26 µs per element in Node and
  150 µs in QuickJS. A record construction plus protocol call costs 4.5 µs in
  Node and 10 µs in QuickJS. V8 gains only 2.2× over the QuickJS interpreter on
  the record loop (5.6× on the pipeline), which suggests that construction and
  dispatch do not optimize well in V8. That is a target for type inference,
  devirtualization and call specialization (light-cljs-210 to 212).
- **Upstream sizes.** Closure reduces the programs that only compute a number
  (the sum, the keyword `case` and the protocol call) to 2.5–5.8 KB. Anything
  that prints, builds a sequence or defines a variadic function is 88–106 KB at
  `:advanced` and 11–56 KB in lite mode. Upstream's 10,240-byte ceiling belongs to `(. js/console (log
  "Hello!"))`; `(println "hi")` measures 88,554 bytes. The Targets table in the
  [design](../docs/advanced-optimization.md) now carries these measured values.

## Pure initializer removal — October 13, 2026

light-cljs-213 makes `--optimize advanced` remove unused definitions whose
initializers only call pure core operations, bind locals, branch, add metadata
or construct program types, and stops retained `js*` from rooting every
top-level form and disabling specialization (Phase 2 steps 6 and 7 of the
[design](../docs/advanced-optimization.md)). `none` and `prepare` output is
unchanged, except for programs that combine `js*` with `try`/`catch`, `case`,
`binding` or `with-redefs`: hygiene now also renames `caught`, `caseValue` and
`installed<n>` away from raw text. The nine earlier ratchet fixtures define
nothing removable and keep their sizes. Two fixtures are new; "before" is
advanced output at 95a5e92, raw and `gzip -9` bytes of
`--compile --optimize LEVEL --text`:

| Fixture | Removed none / prepare / advanced | Raw none / prepare | Advanced before raw / gzip | Advanced raw / gzip | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| pure-defs | 0 / 1 / 6 | 352,356 / 352,278 | 342,431 / 81,861 | 341,693 / 81,691 | 342,000 |
| raw-js | 0 / 0 / 1 | 351,766 / 351,766 | 336,081 / 80,251 | 336,036 / 80,239 | 337,000 |

The saving is the removed definitions' own code (738 bytes for the five
`pure-defs` initializers `prepare` keeps); while every program still reaches
the runtime's `core` hub, no runtime unit goes with them.

In the generated programs' twelve seeds, the 19 `unused-pure-def` items
(`str`, `vector`, `hash-map` and `count` over literals and arithmetic) move from
`survives` (142-279 bytes each) to `residue`: 24-25 bytes each, the `null;`
statement and the name in the namespace bindings, like literal definitions.
240 seeds (`LIGHT_CLJS_GENERATED_COUNT=240`) pass with the tightened row.
`hash-map` and `array-map` rows in `src/core_operations.tsv` now list their
even arities up to eight arguments as `allocates`, since only an odd count
throws (110 `allocates` and 40 `throws` rows now); the `hash-map` macro's
`PersistentHashMap.fromArrays` expansion is checked against that row.

## Direct runtime references — October 13, 2026

light-cljs-204 dismantles the runtime's `core` hub (Phase 1 step 3 of the
[design](../docs/advanced-optimization.md)). Every core operation is now a
runtime binding or a member of one: the `numeric` and `predicates` objects and
the inline members of the old `core` literal became one unit each, the reader
became the `reader` unit, and dynamic Vars, host hooks, `rand` and `flush` live
in the `coreCells` unit, which the hub extends. `--optimize advanced` output
reads `rt$add(1, 2)` instead of `(rt.core.add).call(null, 1, 2)`, so no slice
keeps the hub. Raw and `gzip -9` bytes of `--compile --optimize LEVEL --text`;
"before" is the advanced column at 604a095 (336,000-342,000 ceilings):

| Fixture | Prepare raw / gzip | Advanced raw / gzip | Saved vs prepare | Units kept | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 357,438 / 85,319 | 152,535 / 35,343 | 204,903 | 316 | 153,000 |
| println-hi | 357,325 / 85,247 | 154,002 / 35,543 | 203,323 | 323 | 155,000 |
| vector-printed | 357,350 / 85,256 | 154,064 / 35,563 | 203,286 | 323 | 155,000 |
| map-printed | 357,340 / 85,248 | 154,048 / 35,559 | 203,292 | 323 | 155,000 |
| seq-pipeline | 357,669 / 85,367 | 161,885 / 37,660 | 195,784 | 341 | 162,000 |
| add | 357,278 / 85,239 | 152,375 / 35,274 | 204,903 | 316 | 153,000 |
| rest-fn | 357,622 / 85,376 | 152,264 / 35,274 | 205,358 | 313 | 153,000 |
| keyword-case | 357,910 / 85,393 | 153,733 / 35,669 | 204,177 | 320 | 154,000 |
| record-protocol | 358,231 / 85,566 | 171,476 / 39,747 | 186,755 | 346 | 172,000 |
| pure-defs | 358,789 / 85,667 | 166,838 / 38,430 | 191,951 | 338 | 167,000 |
| raw-js | 358,277 / 85,489 | 153,365 / 35,491 | 204,912 | 316 | 154,000 |

`console-log-sum` drops from 335,242 to 152,535 bytes (80,088 to 35,343
gzip). The shared runtime that `none`, `prepare`, the REPL and the macro host
embed grows by 6,511 bytes (350,927 to 357,438 for `console-log-sum`), so their
ceilings rise to 358,000-359,000: the hub now lists every operation by name
(10,227 bytes), each former inline member is a declaration of its own, and the
type constructors and reader operations are listed explicitly as members.

The runtime has 906 units (356,095 unit bytes). Before, 468 units reached at
least 400 units and every value constructor reached 665; now only the hub and
its `core.seal` effect reach that far (880), and `keyword`, `symbol`,
`vectorFromArray`, `hash`, `str` and `print` each reach 299 units (147,244
bytes). That remaining edge is `builtinValue → builtinType → builtinTypes`,
whose attached prototype and `EMPTY` effects require the collection, hashing
and printing machinery (step 4). `add` reaches 302 units through its variadic
arity effect (`add.variadic → foldSeq → seq`), and the namespace registration
adds 14 more (`registerReaderNamespace`, `theNs`; step 5); without those, the
`(+ 1 2)` slice would be `add` itself, 234 bytes. The
[retention report](results/retention-2026-10-13/console-log-sum.txt) and the
[unit table](results/runtime-units-2026-10-13/units.tsv) are checked in;
reproduce them with the commands of the two sections above.

## Demand-driven runtime effects — October 13, 2026

light-cljs-205 makes the runtime's load-time effects demand-driven (Phase 1
step 4 of the [design](../docs/advanced-optimization.md)). The `builtinTypes`
registry and its two attached effects (prototype patching for every builtin
type and the `EMPTY` constants) became one prototype setup (`.type`), one
members unit (`.members`) and, for six types, one statics unit (`.statics`) per
type, each attached to the type's constructor. The variadic arity of the 24
`variadicCore` operations, the comparisons and the bit operations are
`install:` units, which a slice keeps only where the operation's value
escapes: `rt$add(1, 2)` keeps the 234-byte `add` unit, `(apply + xs)` keeps
`add.variadic` and the sequence machinery behind it. Every `coreMethod`
dispatcher registration is listed by the manifest, and an `extend-type` of a
core protocol method keeps its dispatcher. Raw and `gzip -9` bytes of
`--compile --optimize LEVEL` output; "before" is the advanced output at
6f10f51:

| Fixture | Prepare raw / gzip | Advanced before | Advanced after | Saved | Units kept | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 359,238 / 85,273 | 152,535 / 35,364 | 136,918 / 31,500 | 15,617 | 341 | 137,000 |
| println-hi | 359,125 / 85,197 | 154,002 / 35,559 | 138,655 / 31,777 | 15,347 | 350 | 139,000 |
| vector-printed | 359,150 / 85,211 | 154,064 / 35,583 | 138,717 / 31,799 | 15,347 | 350 | 139,000 |
| map-printed | 359,140 / 85,200 | 154,048 / 35,576 | 138,701 / 31,791 | 15,347 | 350 | 139,000 |
| seq-pipeline | 359,469 / 85,318 | 161,885 / 37,678 | 155,103 / 35,705 | 6,782 | 384 | 156,000 |
| add | 359,078 / 85,182 | 152,375 / 35,283 | 136,758 / 31,417 | 15,617 | 341 | 137,000 |
| rest-fn | 359,422 / 85,321 | 152,264 / 35,287 | 136,917 / 31,482 | 15,347 | 340 | 137,000 |
| keyword-case | 359,710 / 85,344 | 153,733 / 35,687 | 138,033 / 31,823 | 15,700 | 344 | 139,000 |
| record-protocol | 360,031 / 85,518 | 171,476 / 39,768 | 164,486 / 37,626 | 6,990 | 387 | 165,000 |
| pure-defs | 360,589 / 85,615 | 166,838 / 38,445 | 160,123 / 36,423 | 6,715 | 381 | 161,000 |
| raw-js | 360,077 / 85,433 | 153,365 / 35,503 | 137,748 / 31,640 | 15,617 | 341 | 138,000 |
| `[1 2 3]` | 359,077 / 85,183 | 151,912 / 35,185 | 136,565 / 31,382 | 15,347 | 340 | - |
| `{}` | 359,067 / 85,178 | 151,896 / 35,182 | 136,549 / 31,379 | 15,347 | 340 | - |
| `(apply + [1 2 3])` with `<` and `reduce` | 359,384 / 85,297 | 153,301 / 35,550 | 145,155 / 33,330 | 8,146 | 355 | - |

The `add` root of `console-log-sum` now keeps one unit; the 338 units after it
are the namespace registration's (`registerReaderNamespace` reaches `symbol`,
step 5), as the [retention report](results/effects-2026-10-13/console-log-sum.txt)
shows. Any value constructor still reaches 326 units (131,897 bytes): generic
operations (`seq`, `withMeta`, `conj`, `hash`, the printer behind `toString`)
reference every collection constructor, so a program that builds one value
keeps all collection types and their members. What step 4 removes from that
closure is the rest of the old `builtinTypes` effects: eductions, delays,
transients, chunk buffers, UUIDs and the `reduce`/transducer machinery that
only their members needed, and every variadic arity no escaping value needs.
`seq-pipeline`, `record-protocol` and `pure-defs` reach some of that through
their own operations, so they save less.

The shared runtime that `none`, `prepare`, the REPL and the macro host embed
grows by 1,800 bytes (357,438 to 359,238 for `console-log-sum`), so their
ceilings rise to 360,000-361,000: each type's prototype setup is a statement
of its own (`builtinType(ListValue, sequencePrototype);`), the field accessors
are one `publicFields` call per type, and the comparison and bit-operation
variadic arities are separate statements. The runtime has 986 units (357,895
unit bytes): 70 effect units attached to other units' bindings, 70 with
effects on their own bindings (58 of them the dispatcher registrations, now
declared) and 32 installs. The
[unit table](results/effects-2026-10-13/units.tsv) lists each unit's reach as
a value root; reproduce both files with the commands of the sections above.

## Keyword and symbol constant table — October 13, 2026

light-cljs-208 implements Phase 1 step 7 of the
[design](../docs/advanced-optimization.md): `--optimize advanced` declares
each distinct keyword and quoted symbol literal once,
`const k0 = rt$keyword(null, "a");`, after the runtime slice. Below
`Advanced`, every literal site still calls `rt.keyword` on each evaluation, but
the runtime's `keyword` now interns one keyword per namespace and name, so
literal keywords are no longer allocated and hashed again on each
evaluation. Keyword `case` at `Advanced` switches on `rt$keywordFqn(caseValue)`
instead of printing the keyword. The new ratchet fixture `keyword-lookups`
looks up keyword keys in literal maps, both with `get` and by calling the
keyword, and runs a keyword `case`:

```clojure
(defn score [m] (+ (get m :a 0) (get m :b 0) (:c m 0) (get m :ns/d 0)))
(defn kind [k] (case k :x 1 :y 2 0))
(reduce + (map score [{:a 1 :b 2} {:c 3 :ns/d 4} {:a 5 :c (kind :y)}]))
```

`--levels` gains the same program as a third steady-state workload, which
calls `score` on two fresh maps and runs `kind` n = 2,000 times per call.

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL --text`
after rebasing onto light-cljs-205; "before" is master `0984a09`:

| Fixture | Prepare before | Prepare after | Advanced before | Advanced after | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 359,238 / 85,252 | 359,828 / 85,438 | 136,918 / 31,479 | 137,360 / 31,601 | 138,000 |
| keyword-case | 359,710 / 85,326 | 360,300 / 85,512 | 138,033 / 31,805 | 138,550 / 31,948 | 139,000 |
| record-protocol | 360,031 / 85,497 | 360,621 / 85,684 | 164,486 / 37,605 | 164,928 / 37,726 | 165,000 |
| pure-defs | 360,589 / 85,600 | 361,179 / 85,786 | 160,123 / 36,408 | 160,582 / 36,539 | 161,000 |
| keyword-lookups | 360,202 / 85,429 | 360,792 / 85,615 | 148,119 / 34,044 | 148,556 / 34,199 | 149,000 |

The shared runtime grows by 590 bytes: the intern table, the interning
`keyword`, `freshKeyword` for keywords built from data, and `keywordFqn` and
its export. Every `Advanced` slice that reaches `keyword` grows by 442 bytes,
and every slice does today, because the namespace registration's `symbol`
reaches the printer. The table itself is about size-neutral in these small
programs: a declaration costs about as much as the per-site construction it
replaces when a literal occurs once or twice. `keyword-case` also keeps the
`keywordFqn` unit. Every other fixture grows by the same 590 bytes below
`Advanced` and by 442 bytes at it, and no ceiling rises by more than 1,000.

**Startup and steady state.** These numbers come from `--levels` runs on the
pre-205 base `6f10f51`, before and after this change, one directly after the
other. Both runs used the new fixture and workload, and the "before" build
came from an exported tree. The machine was busy: the one-minute load average
had a median of 5.7 and 7.4. Every table is in
[before](results/keyword-constants-2026-10-13/before/summary.md) and
[after](results/keyword-constants-2026-10-13/after/summary.md), next to their
`samples.csv`.

Steady state, median ms per call:

| Workload | n | Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| keyword-lookups | 2,000 | none | 39.96 | 20.80 | 135.35 | 121.27 |
| keyword-lookups | 2,000 | prepare | 39.50 | 20.22 | 135.24 | 120.57 |
| keyword-lookups | 2,000 | advanced | 37.10 | 19.70 | 133.51 | 114.55 |
| seq-pipeline | 200 | advanced | 5.25 | 5.20 | 29.73 | 30.21 |
| record-protocol | 2,000 | advanced | 9.50 | 9.59 | 20.18 | 20.29 |

In Node the keyword workload takes half as long at every level, because a
literal keyword is now one object whose hash is cached. Below `Advanced` that
object comes from the intern table; at `Advanced` it comes from the constant
table. QuickJS gains 10% below `Advanced` and 14% at `Advanced`, where no
lookup is left. The workloads without keyword literals in their loops do not
change.

Parse plus first execution, median ms, Node / QuickJS:

| Fixture | none before | none after | advanced before | advanced after |
| --- | ---: | ---: | ---: | ---: |
| keyword-case | 6.32 / 21.10 | 6.19 / 21.47 | 3.14 / 7.83 | 3.09 / 7.67 |
| keyword-lookups | 7.28 / 21.88 | 7.32 / 21.80 | 4.39 / 8.09 | 4.31 / 8.35 |
| pure-defs | 7.47 / 21.48 | 7.48 / 21.45 | 3.65 / 8.65 | 3.62 / 8.56 |
| record-protocol | 6.75 / 21.47 | 6.60 / 21.67 | 3.63 / 9.06 | 3.69 / 8.88 |

Startup does not move beyond the noise. Each of these programs constructs
only a few keywords once, and the runtime is the same size apart from 590
bytes.

Interning keywords built from data was measured and rejected. In QuickJS, a
weakly held intern table gives no reuse: a keyword built from a string and
dropped is freed at once, so the next build starts again. In a
micro-benchmark that took about twice as long as allocating the keyword, and
every freed keyword also left a `FinalizationRegistry` job that the sessions
never run. A strong table would grow with the data. The `keyword` function,
the reader and keywordized keys therefore still allocate fresh keywords, as
upstream does.
