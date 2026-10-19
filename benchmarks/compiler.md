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
about three minutes. `--only a,b,...` (light-cljs-220) measures only the named
fixtures and steady-state workloads, for a pellet whose change reaches a few of
them. It writes `samples.csv` (every sample, with the one-minute
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

## Compile-time units and conditional namespace registration — October 13, 2026

light-cljs-206 implements Phase 1 step 5 of the
[design](../docs/advanced-optimization.md). The manifest marks 24 units
(29,220 bytes) `compile-time`: the compiler form bridge (11,645), the reader's
core name table (9,366), `inspect`, declaration demand and transactions, the
namespace registry, the macro reader context and the `eval` stub. An
`--optimize advanced` program keeps them, registers its namespaces and switches
`*ns*` at namespace entries only when it reaches reflection (`*ns*`, `find-ns`,
`the-ns`, `all-ns`, `create-ns`, the reader, `eval`, macros defined in the
program, quoted `#js` literals). Before, every slice kept the registration,
whose `symbol` reaches the generic core. The declaration hooks on protocol,
type and multimethod paths are plain functions in application slices, and the
reader builds `#js` literals without the bridge. Raw and `gzip -9` bytes of
`--compile --optimize LEVEL --text` output; "before" is master `d7ae574`:

| Fixture | Prepare before | Prepare after | Advanced before | Advanced after | Saved | Units kept |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 359,828 / 85,438 | 360,411 / 85,664 | 137,360 / 31,601 | 713 / 417 | 136,647 | 1 |
| println-hi | 359,715 / 85,368 | 360,298 / 85,594 | 139,097 / 31,880 | 134,321 / 30,759 | 4,776 | 337 |
| vector-printed | 359,740 / 85,377 | 360,323 / 85,603 | 139,159 / 31,900 | 134,383 / 30,777 | 4,776 | 337 |
| map-printed | 359,730 / 85,370 | 360,313 / 85,595 | 139,143 / 31,896 | 134,367 / 30,772 | 4,776 | 337 |
| seq-pipeline | 360,059 / 85,487 | 360,642 / 85,712 | 155,545 / 35,807 | 150,769 / 34,666 | 4,776 | 371 |
| add | 359,668 / 85,360 | 360,251 / 85,586 | 137,200 / 31,529 | 553 / 331 | 136,647 | 1 |
| rest-fn | 360,012 / 85,494 | 360,595 / 85,722 | 137,359 / 31,592 | 132,583 / 30,448 | 4,776 | 327 |
| keyword-case | 360,300 / 85,512 | 360,883 / 85,740 | 138,550 / 31,948 | 133,748 / 30,811 | 4,802 | 331 |
| record-protocol | 360,621 / 85,684 | 361,204 / 85,912 | 164,928 / 37,726 | 159,603 / 36,495 | 5,325 | 374 |
| pure-defs | 361,179 / 85,786 | 361,762 / 86,013 | 160,582 / 36,539 | 155,421 / 35,357 | 5,161 | 368 |
| keyword-lookups | 360,792 / 85,615 | 361,375 / 85,843 | 148,556 / 34,199 | 143,721 / 33,069 | 4,835 | 347 |
| raw-js | 360,667 / 85,612 | 361,250 / 85,837 | 138,190 / 31,752 | 1,404 / 567 | 136,786 | 1 |
| `(str *ns*)` | 359,681 / 85,358 | 360,264 / 85,585 | 136,984 / 31,482 | 136,516 / 31,462 | 468 | 341 |
| `(read-string "{:a 1}")` | 359,679 / 85,364 | 360,262 / 85,590 | 213,915 / 51,301 | 201,922 / 48,180 | 11,993 | 402 |

`console-log-sum` is now the `add` unit and the call
([retention report](results/compile-time-2026-10-13/console-log-sum.txt)),
well under the 10 KB design target; `(+ 1 2)` and `raw-js` likewise. Every
program that builds a value still keeps the generic core of about 330 units
(132 KB) that the collection constructors reach (step 6 and rapid type
analysis), and saves the 4.8-5.3 KB of the registry and the registration. A
reflective program such as `(str *ns*)` keeps the registration and saves only
what the plain hooks save
([retention report](results/compile-time-2026-10-13/current-namespace.txt)).
A reader program no longer keeps the compiler form bridge, which saves 12 KB;
it still keeps `readerCoreNames` and `inspect`, because the EDN and code
readers are one unit. The generated dead-code programs' unreferenced
namespaces, unused literal and pure-call defs, unused defns and keywords
change from `residue` to `removed`.

The shared runtime that `none`, `prepare`, the REPL and the macro host embed
grows by 583 bytes, so the ceilings of eight fixtures rise by 1,000: the
plain hooks and their one-line comments, the journaling versions that
`declarationTransaction` installs, and `readerJsLiterals`. The `advanced`
ceilings fall to the sizes above, rounded up.

## Demand-driven printing — October 13, 2026

light-cljs-207 implements Phase 1 step 6 of the
[design](../docs/advanced-optimization.md): the printer installs collection
and `ExceptionInfo` `toString` when it loads, so an `--optimize advanced`
program keeps it only when it can print (`str`, `pr-str`, `prn`, `println`,
`clj->js`, ...). Before, every collection prototype's `toString` required the
printer, and so did the runtime's error messages that show a value (`No value
supplied for key`, `Cannot compare`, multimethod errors), the emitter's `case`
and `assert` messages, and the `-pr-writer` methods of records and eductions.
Those now format values without it. Raw and `gzip -9` bytes of
`--compile --optimize LEVEL --text` output; "before" is master `d94515b`:

| Fixture | Prepare before | Prepare after | Advanced before | Advanced after | Saved | Units kept |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 360,411 / 85,677 | 361,495 / 86,022 | 713 / 431 | 713 / 431 | 0 | 1 |
| println-hi | 360,298 / 85,607 | 361,382 / 85,951 | 134,321 / 30,773 | 134,836 / 30,947 | -515 | 339 |
| vector-printed | 360,323 / 85,616 | 361,407 / 85,961 | 134,383 / 30,791 | 134,898 / 30,963 | -515 | 339 |
| map-printed | 360,313 / 85,608 | 361,397 / 85,952 | 134,367 / 30,786 | 134,882 / 30,958 | -515 | 339 |
| seq-pipeline | 360,642 / 85,725 | 361,726 / 86,070 | 150,769 / 34,680 | 138,105 / 31,675 | 12,664 | 343 |
| add | 360,251 / 85,599 | 361,335 / 85,943 | 553 / 345 | 553 / 345 | 0 | 1 |
| rest-fn | 360,595 / 85,735 | 361,679 / 86,078 | 132,583 / 30,462 | 119,919 / 27,408 | 12,664 | 299 |
| keyword-case | 360,883 / 85,753 | 361,967 / 86,096 | 133,748 / 30,825 | 121,084 / 27,796 | 12,664 | 303 |
| record-protocol | 361,204 / 85,925 | 362,288 / 86,268 | 159,603 / 36,509 | 149,461 / 33,808 | 10,142 | 352 |
| pure-defs | 361,762 / 86,027 | 362,846 / 86,371 | 155,421 / 35,372 | 145,279 / 32,717 | 10,142 | 346 |
| keyword-lookups | 361,375 / 85,857 | 362,459 / 86,200 | 143,721 / 33,084 | 131,057 / 30,087 | 12,664 | 319 |
| raw-js | 361,250 / 85,851 | 362,334 / 86,193 | 1,404 / 582 | 1,404 / 582 | 0 | 1 |
| `[1 2 3]` | 360,250 / 85,597 | 361,334 / 85,942 | 132,231 / 30,353 | 119,567 / 27,309 | 12,664 | 299 |
| `(pr-str [1 2 3])` | 360,278 / 85,609 | 361,362 / 85,954 | 132,919 / 30,471 | 133,434 / 30,677 | -515 | 332 |
| `(str [1 2 3])` | 360,276 / 85,609 | 361,360 / 85,954 | 132,252 / 30,365 | 133,014 / 30,610 | -762 | 330 |
| `(defmulti m :t)`, one method, one call | 360,782 / 85,713 | 361,866 / 86,056 | 154,930 / 35,644 | 144,713 / 32,934 | 10,217 | 362 |
| `ex-info` thrown and caught | 360,455 / 85,663 | 361,539 / 86,005 | 132,779 / 30,524 | 120,894 / 27,720 | 11,885 | 303 |

`[1 2 3]` unprinted is 119,567 bytes and printed with `pr-str` 133,434: the
printer is 33 units, 13.8 KB
([unprinted](results/print-2026-10-13/vector-unprinted.txt) and
[printed](results/print-2026-10-13/vector-printed.txt) retention reports).
Records and defmulti programs save less because they shared some of the
printer's units with their own. A program that prints grows by 515 bytes:
`printHooks`, `printNumber`, `displayStr` and the install, less `str` and its
variadic arity, which the error paths kept before. `(str [1 2 3])` keeps `str`
again and grows by 762.

What a value-building program still keeps, about 300 units (119 KB), is one
strongly connected component: `seq`, `count`, `=`, `hash`, `meta`, `conj` and
`assoc` dispatch over every builtin collection, so each references every
collection constructor, whose prototype and members reference those
operations back. Every one of them reaches the same 299 units. Cutting it
needs rapid type analysis (Phase 2).

The shared runtime that `none`, `prepare`, the REPL and the macro host embed
grows by 1,084 bytes, so every fixture's `none` and `prepare` ceiling rises
by 1,000: `printHooks` and `printNumber`, the install unit, `displayStr` and
its export, and the longer `toString` and message call sites. The `advanced`
ceilings fall to the sizes above, rounded up.

## Type inference — October 13, 2026

light-cljs-210 implements Phase 2 step 3 of the
[design](../docs/advanced-optimization.md): `--optimize advanced` infers a
type for every expression of the prepared program (`src/inference.rs`) and
emits `if` tests without `truth()` where JavaScript truthiness agrees,
`(and a b)` as `(a && b)`, arithmetic, comparisons and `inc`/`dec`/`nil?`/
`zero?`/... as JavaScript operators, and typed `=`, `not`, `count`, `nth` and
record lookups without calling the runtime. Output that never calls `truth`
leaves out its declaration. Below `Advanced` nothing changes.

**Sizes**, raw and `gzip -9` bytes of the ratchet fixtures at `advanced`;
"before" is master `857471a` (after light-cljs-207). `none` and `prepare` do
not change.

| Fixture | Advanced before | Advanced after | Advanced ceiling |
| --- | ---: | ---: | ---: |
| console-log-sum | 713 / 431 | 192 / 161 | 200 |
| add | 553 / 345 | 32 / 52 | 100 |
| raw-js | 1,404 / 582 | 856 / 319 | 900 |
| keyword-case | 121,084 / 27,796 | 120,164 / 27,472 | 121,000 |
| record-protocol | 149,461 / 33,808 | 148,802 / 33,565 | 149,000 |
| pure-defs | 145,279 / 32,717 | 145,151 / 32,679 | 146,000 |
| keyword-lookups | 131,057 / 30,087 | 130,993 / 30,060 | 131,000 |
| println-hi | 134,836 / 30,947 | 134,775 / 30,925 | 135,000 |
| rest-fn | 119,919 / 27,408 | 119,858 / 27,384 | 120,000 |
| seq-pipeline | 138,105 / 31,675 | 138,044 / 31,652 | 139,000 |

`(js/console.log (+ 1 2))` reads no runtime any more: it is
`(host("console")["log"])((1 + 2))` inside the output closure, below the
design's 1,000-byte target for that row. `add` and `raw-js` likewise lose the
`add` unit. Elsewhere the slice loses the units of the lowered operations
(`add`, `multiply` and `numericFold` in `keyword-case`) and the output its
`truth` prelude; the programs that build values still keep the collection and
printing core.

**Startup and steady state.** `--levels` runs on master `d94515b`, before
light-cljs-207 landed, from an exported tree and then with this change, one
directly after the other. The machine was busy, more
so during the "before" run (median one-minute load 8.5 against 4.6), so small
differences favor "after". All tables are in
[before](results/type-inference-2026-10-13/before/summary.md) and
[after](results/type-inference-2026-10-13/after/summary.md), next to their
`samples.csv`.

Parse plus first execution at `advanced`, median ms, Node / QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| console-log-sum | 0.670 / 0.046 | 0.570 / 0.020 |
| add | 0.145 / 0.036 | 0.110 / 0.008 |
| raw-js | 0.168 / 0.080 | 0.135 / 0.051 |
| record-protocol | 3.236 / 8.475 | 3.191 / 8.121 |
| pure-defs | 3.242 / 8.215 | 2.929 / 7.842 |

Steady state at `advanced`, median ms per call:

| Workload | n | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: | ---: |
| seq-pipeline | 200 | 5.54 | 5.40 | 29.62 | 29.15 |
| record-protocol | 2,000 | 9.74 | 9.94 | 20.52 | 18.93 |
| keyword-lookups | 2,000 | 19.26 | 18.70 | 108.35 | 108.29 |

Only `record-protocol` changes beyond the noise, in QuickJS: its `area`
multiplies with `*` instead of calling the runtime's `multiply`, which saves
8%. Node inlines the small runtime functions itself. The other workloads spend
their time in the sequence and map runtime, which the lowerings do not touch;
their parameters are untyped, so `=`, `count` and `nth` on them keep the
runtime call.

## Constant collection pool — October 13, 2026

light-cljs-218 implements tiers 1 and 2 of "Constant collection literals" in
the [design](../docs/advanced-optimization.md). At every level the empty
literals `[]`, `{}`, `#{}` and `()` read the runtime's `EMPTY` singletons. At
`--optimize advanced` each distinct constant collection literal, reader
metadata on one and `#uuid` literal is a constant table entry, built once from
the entries of its parts, `const k1 = rt$vectorFromArray([1, k0]);`, and
constants of 1,024 or more nodes are lazy cells. The new ratchet fixture
`constant-literals` builds a map with a dynamic value and constant parts:

```clojure
(defn row [i] {:id i :tags #{:a :b} :pos [1 2] :empty []
               :meta {:kind :row :dims [1 2 3]}})
(reduce + (map (fn [i] (let [r (row i)]
                         (+ (count (:pos r)) (count (get-in r [:meta :dims])))))
               (range 10)))
```

`--levels` gains it as a fourth steady-state workload, which calls `row`
n = 2,000 times per call and also counts `:tags`.

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL --text`;
"before" is master `21b1abf` (after light-cljs-206, 207 and 210). Fixtures not
listed do not change.

| Fixture | None before | None after | Advanced before | Advanced after | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | ---: |
| vector-printed | 361,407 / 85,946 | 361,407 / 85,946 | 134,837 / 30,944 | 134,854 / 30,950 | 135,000 |
| map-printed | 361,397 / 85,939 | 361,410 / 85,941 | 134,821 / 30,938 | 134,856 / 30,938 | 135,000 |
| keyword-lookups | 362,459 / 86,105 | 362,459 / 86,105 | 130,993 / 30,060 | 131,027 / 30,074 | 132,000 |
| constant-literals | 362,228 / 86,131 | 362,236 / 86,137 | 132,524 / 30,466 | 132,677 / 30,500 | 133,000 |

The pool does not shrink these programs: a table declaration costs a few more
bytes than the construction it replaces when a literal occurs once, and the
fixtures have few literals. `map-printed` reads `PersistentArrayMap.EMPTY`
instead of calling `mapFromArray([])`, 13 bytes more below `Advanced`. The
runtime does not change. The `keyword-lookups` ceiling rises from 131,000 to
132,000, because its two constant maps become declarations (34 bytes) on a
size that was 7 bytes under the ceiling; `constant-literals` gets
[363,000, 363,000, 133,000]. Its advanced table has 13 entries (3 composite)
in 515 bytes, which `--size-report` now prints under its summary line.

**Steady state.** `--levels` on master `d7ae574` (exported tree, with the new
fixture and workload) and on this change before it was rebased onto
light-cljs-206, 207 and 210, one directly after the other. The
one-minute load average had a median of 7.4 and 5.3. Every table is in
[before](results/constant-pool-2026-10-13/before/summary.md) and
[after](results/constant-pool-2026-10-13/after/summary.md), next to their
`samples.csv`. Median ms per call:

| Workload | n | Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| constant-literals | 2,000 | none | 59.39 | 55.07 | 308.04 | 304.75 |
| constant-literals | 2,000 | prepare | 58.14 | 54.98 | 308.65 | 305.36 |
| constant-literals | 2,000 | advanced | 58.21 | 43.47 | 292.25 | 271.11 |
| keyword-lookups | 2,000 | advanced | 20.01 | 19.39 | 114.60 | 114.04 |
| record-protocol | 2,000 | advanced | 9.75 | 9.34 | 20.80 | 20.42 |
| seq-pipeline | 200 | advanced | 5.46 | 5.46 | 30.31 | 30.13 |

At `Advanced` the constant-literals workload takes 25% less time in Node and
7% less in QuickJS: each iteration still builds the outer map, whose `:id` is
dynamic, but no longer its set, vectors, nested map and the `get-in` path.
Below `Advanced` the workload moves by about as much as the others, which ran
under less load the second time: only the empty vector is shared there. Parse
plus first execution does not change beyond that noise at any level.

**Lazy threshold.** A lazy cell, `let c<n>; const k<n> = () => c<n> ??=
<initializer>;`, builds a constant on its first read instead of while the
output loads. Programs of 60 or 10 retained functions, each returning a
distinct vector of numbers of the given size, were compiled at `Advanced`
with every constant eager or every constant lazy, and measured by parse plus
first execution in Node (`benchmarks/support/compiler_levels.js startup`, 21
or 31 fresh processes) and in a fresh QuickJS `Evaluator` (as `--levels`
does). In one variant the program only returns the functions, so startup
reads no constant; in the other it calls each once. Medians, ms; all rows are
in [lazy-threshold.tsv](results/constant-pool-2026-10-13/lazy-threshold.tsv):

| Constants | Nodes | Read at startup | Node eager | Node lazy | QuickJS eager | QuickJS lazy |
| ---: | ---: | --- | ---: | ---: | ---: | ---: |
| 60 | 17 | no | 3.380 | 3.434 | 7.863 | 8.095 |
| 60 | 17 | yes | 3.661 | 3.970 | 8.165 | 8.606 |
| 60 | 257 | no | 4.137 | 4.035 | 13.084 | 12.481 |
| 60 | 257 | yes | 4.381 | 5.353 | 13.375 | 13.841 |
| 10 | 1,025 | no | 3.606 | 3.548 | 10.644 | 10.126 |
| 10 | 1,025 | yes | 3.714 | 4.362 | 10.745 | 10.731 |
| 10 | 4,097 | no | 5.127 | 4.838 | 21.003 | 18.807 |
| 10 | 4,097 | yes | 5.226 | 7.704 | 21.065 | 21.006 |

A lazy cell saves QuickJS the construction of a constant that startup does
not read, about 50 µs per 1,025-node vector and 220 µs per 4,097-node one,
and costs it nothing measurable when startup does read it. V8 gains little
when startup does not read the constant, and when it does, it parses the
cell's body a second time on the first call, about 65 µs per 1,025-node
vector. Below a few hundred nodes the closure and call cost more than they
save in both engines. The threshold is therefore 1,024 nodes, where QuickJS
clearly gains and the V8 cost is bounded by the size of rare, large literals.
Wrapping the cell in parentheses, `(() => ...)`, makes V8 compile it eagerly
and removes its second parse, but QuickJS then scans the whole body twice to
find the arrow, which cost it about 2 ms on the 4,097-node program, so the
cell is a plain arrow function.

## Rapid type analysis of program types — October 13, 2026

light-cljs-211 implements Phase 2 step 4 of the
[design](../docs/advanced-optimization.md) for program types and protocols.
At `--optimize advanced`, `src/rta.rs` keeps a `deftype`, `defrecord`,
`reify` or `extend-type` method only when an instance can run it. It drops
the types nothing constructs, the protocol values and dispatchers nothing
reads, and unread map factories. It also devirtualizes protocol calls on
receivers of one program type into slot reads. Three runtime changes support
this:

- The type machinery (`makeType`, `installMethods`, `reify`,
  `protocolMethod`) reaches record methods, variadic `applyTo`, metadata
  dispatch and variadic protocol arguments only through `typeHooks`.
- Each record method is its own unit, kept by the new manifest clause
  `effect: recordMethods when: <operations>` only where an operation that
  dispatches it is kept.
- A type's `getBasis` exists only when the program names it.

The new ratchet fixture `three-records` has three records implementing one
protocol, of which only one is constructed:

```clojure
(defprotocol Shape (area [s]) (perimeter [s]))
(defrecord Square [side] Shape (area [_] (* side side)) (perimeter [_] (* 4 side)))
(defrecord Circle [r] Shape (area [_] (* 3 r r)) (perimeter [_] (* 6 r)))
(defrecord Tri [a b c] Shape (area [_] (* a b)) (perimeter [_] (+ a b c)))
(area (->Square 3))
```

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL --text`.
"Before" is master `ecbb3c2` (after light-cljs-218). `prepare` equals `none`
for every fixture but `pure-defs`.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 361,495 / 86,009 | 362,755 / 86,290 | 192 / 161 | 192 / 161 | 363,000 / 363,000 / 200 |
| add | 361,335 / 85,930 | 362,595 / 86,211 | 32 / 52 | 32 / 52 | 363,000 / 363,000 / 100 |
| raw-js | 362,334 / 86,179 | 363,594 / 86,461 | 856 / 319 | 856 / 319 | 364,000 / 364,000 / 900 |
| record-protocol | 362,288 / 86,255 | 363,548 / 86,537 | 148,802 / 33,565 | 11,637 / 3,786 | 364,000 / 364,000 / 12,000 |
| pure-defs | 362,924 / 86,366 | 364,184 / 86,646 | 145,151 / 32,679 | 11,471 / 3,715 | 365,000 / 365,000 / 12,000 |
| three-records | 364,425 / 86,556 | 365,685 / 86,837 | 150,433 / 33,806 | 11,642 / 3,780 | 366,000 / 366,000 / 12,000 |
| println-hi | 361,382 / 85,938 | 362,642 / 86,219 | 134,775 / 30,925 | 134,840 / 30,943 | 363,000 / 363,000 / 135,000 |
| vector-printed | 361,407 / 85,948 | 362,667 / 86,228 | 134,854 / 30,950 | 134,919 / 30,967 | 363,000 / 363,000 / 135,000 |
| map-printed | 361,410 / 85,941 | 362,670 / 86,222 | 134,856 / 30,938 | 134,921 / 30,957 | 363,000 / 363,000 / 135,000 |
| seq-pipeline | 361,726 / 86,057 | 362,986 / 86,338 | 138,044 / 31,652 | 138,109 / 31,669 | 363,000 / 363,000 / 139,000 |
| rest-fn | 361,679 / 86,065 | 362,939 / 86,346 | 119,858 / 27,384 | 119,923 / 27,405 | 363,000 / 363,000 / 120,000 |
| keyword-case | 361,967 / 86,083 | 363,227 / 86,364 | 120,164 / 27,472 | 120,229 / 27,493 | 364,000 / 364,000 / 121,000 |
| keyword-lookups | 362,459 / 86,186 | 363,719 / 86,467 | 131,027 / 30,074 | 131,092 / 30,092 | 364,000 / 364,000 / 132,000 |
| constant-literals | 362,236 / 86,137 | 363,496 / 86,418 | 132,677 / 30,500 | 132,742 / 30,518 | 364,000 / 364,000 / 133,000 |

The record programs keep 24 runtime units instead of 350 (`record-protocol`):
- `makeType` and `installMethods`, without the generic core;
- the record method table and its install, with no record method, since
  nothing dispatches one: `(:x p)` reads the field;
- for `record-protocol` and `three-records`, no dispatcher at all, because
  the call reads `["cljs$user$Shape$area$arity$1"]` from the receiver.

`pure-defs` loses `getBasis`, whose vector of symbols kept the collection
core, and the unread `map->Point` with `recordFromMap`.

Programs that keep the generic collection core grow by 65 bytes, the
`typeHooks` object and the `fnApplyTo` install that `installMethods` needs
there. The shared runtime grows by 1,260 bytes:
- the hooks and `metadataMethod`;
- the record methods as fifteen units;
- two exports (`recordMethods` and `metadataMethod`, which the emitter keeps
  by name).

The `none` and `prepare` ceilings rise by 1,000 for that growth. A program
that builds a collection still keeps the 301-unit core (119 KB); its builtin
types are light-cljs-227.

**Startup and steady state.** `--levels --samples 9 --warmups 1` on master
`ecbb3c2` (exported tree) and on this change, one directly after the other,
under the load of other builds: the median one-minute load average was 4.1
and 3.8. Every table is in
[before](results/rapid-types-2026-10-13/before/summary.md) and
[after](results/rapid-types-2026-10-13/after/summary.md), next to their
`samples.csv`. The table gives parse plus first execution at `advanced`,
median ms, Node / QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| record-protocol | 2.988 / 7.419 | 0.790 / 0.592 |
| pure-defs | 2.837 / 7.284 | 0.797 / 0.576 |
| three-records | — | 0.809 / 0.591 |
| vector-printed | 3.649 / 6.674 | 3.649 / 6.832 |

Steady state of the record-protocol workload (`(area (->Rect i 2))` in a
loop, n = 2,000), median µs per call:

| Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: |
| none | 9,600 | 9,589 | 21,233 | 20,120 |
| advanced | 9,874 | 9,506 | 19,203 | 16,500 |

The record programs start four times faster in Node and twelve times faster
in QuickJS. QuickJS no longer evaluates the 140 KB of runtime they used to
embed. In QuickJS the devirtualized `area` call saves another 9% at
`advanced` beyond the 5% that `none` moved between the two runs: it reads
the method from the record instead of going through `protocolMethod`'s
dispatcher, with its arity checks. Node inlines the dispatcher either way.
The other fixtures and workloads do not change beyond the noise.

## Constant hashes — October 13, 2026

light-cljs-219 implements tier 3 of "Constant collection literals" in the
[design](../docs/advanced-optimization.md). At every level the runtime's
`hash` now keeps the hash of a vector, set or queue once computed, as it
already did for maps, lists, keywords and symbols; before, a constant vector
or set used as a key was rehashed on every lookup. At `--optimize advanced`
each composite constant of the pool is declared with the hash the compiler
computes for it (`src/value_hash.rs`, a bit-exact port of the runtime's
hashing): `const k6 = rt$hashed(rt$vectorFromArray([1, 2]), 1185709346);`.
The new ratchet fixture and `--levels` workload `constant-keys` looks up a
36-entry hash map keyed by maps, vectors and sets with constant keys:

```clojure
(def table (zipmap (concat (map (fn [i] {:id i :kind :row}) (range 12))
                           (map (fn [i] [:pos i (inc i)]) (range 12))
                           (map (fn [i] #{i :tag}) (range 12)))
                   (range 36)))
(+ (get table {:id 3 :kind :row}) (get table [:pos 5 6]) (get table #{7 :tag})
   (get table {:id 11 :kind :row} 0))
```

**Sizes**, bytes of `--compile --optimize advanced --text`; "before" is
master `fcfd2ae` (after light-cljs-211). Below `Advanced` every fixture is
124 bytes smaller, because `hash` lost its per-kind cache checks
(`pure-defs` at `prepare` falls from 364,106 to 363,982, so its ceiling
becomes 364,000). At `Advanced` a program that embeds `hash` saves 222 bytes
and pays 109 for the `hashed` unit and its binding and 22-24 per composite
constant (`constant-literals` has 5). `console-log-sum`, `add`, `raw-js` and,
since 211, `record-protocol`, `pure-defs` and `three-records` embed neither
and do not change.

| Fixture | Advanced before | Advanced after | Ceiling before | Ceiling after |
| --- | ---: | ---: | ---: | ---: |
| println-hi | 134,840 | 134,618 | 135,000 | 135,000 |
| vector-printed | 134,919 | 134,830 | 135,000 | 135,000 |
| map-printed | 134,921 | 134,699 | 135,000 | 135,000 |
| seq-pipeline | 138,109 | 137,887 | 139,000 | 138,000 |
| rest-fn | 119,923 | 119,701 | 120,000 | 120,000 |
| keyword-case | 120,229 | 120,007 | 121,000 | 121,000 |
| keyword-lookups | 131,092 | 131,026 | 132,000 | 132,000 |
| constant-literals | 132,742 | 132,746 | 133,000 | 133,000 |
| constant-keys | 124,785 | 124,762 | — | 125,000 |

`constant-keys` gets [364,000, 364,000, 125,000]. The 13-entry table of the
allocation fixture in `tests/compiler/constant_pool.rs` grows from 515 to 583
bytes for its 3 composite entries.

**Steady state and startup.** `--levels --samples 9 --warmups 1` on the
exported master tree `ecbb3c2` with the new fixture and workload, then on
this change before it was rebased onto light-cljs-211, one directly after the
other; the one-minute load average had a median of
3.65 and 2.81. Every table is in
[before](results/constant-hashes-2026-10-13/before/summary.md) and
[after](results/constant-hashes-2026-10-13/after/summary.md), next to their
`samples.csv`. The workload ran with n = 2,000, which takes about a second
per QuickJS call, so it now runs n = 200. Median ms per call:

| Workload | n | Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| constant-keys | 2,000 | none | 173.12 | 168.98 | 1,039.51 | 1,013.55 |
| constant-keys | 2,000 | prepare | 172.70 | 168.32 | 1,044.85 | 1,010.43 |
| constant-keys | 2,000 | advanced | 147.87 | 106.64 | 949.53 | 692.69 |
| constant-literals | 2,000 | advanced | 41.73 | 40.96 | 257.77 | 251.86 |
| keyword-lookups | 2,000 | advanced | 18.47 | 18.03 | 105.00 | 103.32 |
| record-protocol | 2,000 | advanced | 9.92 | 9.40 | 19.01 | 18.85 |
| seq-pipeline | 200 | advanced | 5.27 | 5.07 | 29.02 | 28.39 |

At `Advanced` the lookups take 28% less time in Node and 27% less in QuickJS:
the pooled vector and set keys are no longer rehashed per lookup (the map
keys' hashes were already cached), and each key's first hash is the embedded
one. What remains is mostly the structural `=` against the stored key. Below
`Advanced` every lookup builds its key, which is hashed once either way, so
those levels move by about as much as the other workloads, 2-4%, which the
lower load of the second run explains. Parse plus first execution, median ms:

| Fixture | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: |
| constant-keys, none | 12.83 | 11.88 | 31.06 | 29.00 |
| constant-keys, advanced | 8.30 | 8.08 | 14.95 | 13.62 |
| constant-literals, advanced | 4.83 | 4.77 | 8.39 | 7.96 |
| keyword-lookups, advanced | 3.55 | 3.41 | 6.79 | 6.33 |
| vector-printed, advanced | 3.63 | 3.50 | 6.72 | 6.30 |

Startup improves by 2-9% at every level, `none` included, so the difference
is mostly the load; seeding the constants' hashes adds no cost that this run
can resolve.

## Direct call shapes — October 13, 2026

light-cljs-212 implements Phase 2 step 5 of the
[design](../docs/advanced-optimization.md), except the hoisting of local
functions. At `--optimize advanced`, `src/call_shapes.rs` finds the functions
that one `def` binds and nothing else assigns. Their calls name the method
the argument count selects:

- `g3(a, b)` for a function with one method;
- `g3_1(a)` for a method entry;
- `g3_2(a, rt$fnRest([b, c], 0))` for a variadic method.

A function whose value nothing else sees is defined as its entries alone,
without the `arguments.length` dispatcher and arity properties. A `do` in
value position, such as every `defn` initializer, is a comma expression
instead of a function call.

The new ratchet fixture `multi-arity` has three functions with two methods
each, which only call each other:

```clojure
(defn clamp ([x] (clamp x 0 10)) ([x lo hi] (max lo (min hi x))))
(defn area ([w] (area w w)) ([w h] (* w h)))
(defn fib ([n] (fib n 0 1)) ([n a b] (if (zero? n) a (recur (dec n) b (+ a b)))))
(+ (clamp 15) (area 3) (area 2 5) (fib 10))
```

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL --text`.
"Before" is master `fcfd2ae` (after light-cljs-211). `none` and `prepare` do
not change, and neither do fixtures without a `defn`.

| Fixture | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | --- |
| multi-arity | 3,579 / 1,025 | 2,040 / 839 | 366,000 / 366,000 / 2,100 |
| raw-js | 856 / 319 | 271 / 133 | 364,000 / 364,000 / 300 |
| keyword-case | 120,229 / 27,493 | 120,124 / 27,480 | 364,000 / 364,000 / 120,000 |
| keyword-lookups | 131,092 / 30,092 | 130,975 / 30,080 | 364,000 / 364,000 / 131,000 |
| constant-literals | 132,742 / 30,518 | 132,691 / 30,508 | 364,000 / 364,000 / 133,000 |

`multi-arity` at `none` is 365,423 / 86,722 bytes. In `raw-js` the function
`add`, with two methods, loses its dispatcher and both function wrappers.
`keyword-case`, `keyword-lookups` and `constant-literals` lose their `defn`
wrappers, 50 to 120 bytes. On top of light-cljs-219, which landed meanwhile
and shrinks the runtime, `keyword-case` is 119,902 bytes and
`keyword-lookups` 130,909, so their ceilings drop to 120,000 and 131,000.

**Call costs.** `cargo bench --bench compiler -- --callees` times one million
calls per sample of each call shape on each kind of function value
([support/callees.js](../benches/support/callees.js)): a function with one
method (`single`), a dispatcher over fixed methods (`multi`) and one with a
variadic method (`variadic`), each called with one argument. 15 timed rounds
after 3 untimed ones, median ns per call, one after the other with the levels
runs below ([callees.md](results/call-shapes-2026-10-13/callees.md)):

| Shape | single, Node | multi, Node | variadic, Node | single, QuickJS | multi, QuickJS | variadic, QuickJS |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `f.call(null, a)` | 4.3 | 7.8 | 7.8 | 40.1 | 151.5 | 145.0 |
| upstream's arity check | 4.3 | 5.0 | 4.5 | 47.9 | 37.3 | 37.1 |
| the method, `m(a)` | 0.5 | 4.3 | 4.2 | 28.8 | 28.5 | 28.7 |

The last row is what a known target now gets: a call of a function with
several methods is five times faster in QuickJS and almost twice as fast in
Node, and one with a single method 28% faster in QuickJS. Node inlines the
plain single-method call entirely here.

The middle row is upstream's `:fn-invoke-direct` shape for unknown callees,
`f.cljs$core$IFn$_invoke$arity$1 ? f.cljs$core$IFn$_invoke$arity$1(a) :
f.call(null, a)`. It skips the dispatcher of a function with several methods,
four times faster in QuickJS. But it costs a failed property read on every
other callee, 19% slower in QuickJS for a single method, which is the shape
of every `fn` literal. It also adds about 60 bytes per call site until Phase
3 mangles the property name, needs a callee it can read twice, and passes the
function as `this`, which `this-as` inside a method would see. Unknown
callees therefore keep `.call(null, ...)`; see "Phase 2 call shape extension
points" in the design.

**Startup and steady state.** `--levels --samples 9 --warmups 1` on master
`fcfd2ae` (exported tree, with this change's `multi-arity` fixture and
workload added) and on this change, one directly after the other. Other
builds ran at the same time: the median one-minute load average was 4.65 in
both runs, with peaks of 9.4 and 8.9. Every table is in
[before](results/call-shapes-2026-10-13/before/summary.md) and
[after](results/call-shapes-2026-10-13/after/summary.md), next to their
`samples.csv`. Parse plus first execution at `advanced`, median ms,
Node / QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| multi-arity | 0.260 / 0.212 | 0.200 / 0.120 |
| raw-js | 0.134 / 0.052 | 0.113 / 0.020 |

The `multi-arity` workload calls `clamp` with one argument and `area` with one
and with two in a loop (n = 50,000), median µs per call:

| Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: |
| none | 6,675 | 6,616 | 147,673 | 145,010 |
| advanced | 3,125 | 146 | 82,619 | 45,512 |

At `advanced` before this change, type inference already wrote the
arithmetic as operators. Calling the methods directly makes the loop 1.8 times
faster in QuickJS, and in Node 21 times faster: V8 now inlines the whole
loop body, where every call used to go through `.call`, the dispatcher's
`switch` and `.apply(this, arguments)`. The other fixtures and workloads do
not change beyond the noise.

## Builtin types — October 13, 2026

light-cljs-227 extends rapid type analysis (Phase 2 step 4 of the
[design](../docs/advanced-optimization.md)) to the runtime's own types, in
the runtime and its manifest. At `--optimize advanced` a program keeps the
collection, keyword and seq types it can build and, of each generic
operation it keeps, only the branches for those types:

- generic operations reach protocol dispatch through `protocolHooks`, which
  only `installMethods`, and so only a program with protocol
  implementations, fills in;
- each type's branch of an operation is a `kindHooks` entry that the units
  building the type install where the operation is kept;
- the factories of several types (`view`, `makeMap`, `makeSet`) take the
  type from their caller.

The new ratchet fixtures `vector-only` and `keyword-only` build a vector and
iterate it, and build a keyword and call its `toString`:

```clojure
(js/console.log (.join (js/Array.from [1 2 3]) " "))
(js/console.log (.toString :a))
```

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL --text`.
"Before" is master `75d9dd0` (after light-cljs-212). `prepare` equals `none`
for every fixture but `pure-defs`, which is 78 bytes smaller there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 362,631 / 86,288 | 375,538 / 89,378 | 192 / 161 | 192 / 161 | 376,000 / 376,000 / 200 |
| add | 362,471 / 86,209 | 375,378 / 89,299 | 32 / 52 | 32 / 52 | 376,000 / 376,000 / 100 |
| raw-js | 363,470 / 86,460 | 376,377 / 89,548 | 271 / 133 | 271 / 133 | 377,000 / 377,000 / 300 |
| multi-arity | 365,299 / 86,721 | 378,206 / 89,809 | 2,040 / 839 | 2,040 / 839 | 379,000 / 379,000 / 2,100 |
| record-protocol | 363,424 / 86,536 | 376,331 / 89,626 | 11,637 / 3,786 | 11,637 / 3,786 | 377,000 / 377,000 / 12,000 |
| pure-defs | 364,060 / 86,645 | 376,967 / 89,733 | 11,471 / 3,715 | 11,471 / 3,715 | 377,000 / 377,000 / 12,000 |
| three-records | 365,561 / 86,836 | 378,468 / 89,926 | 11,642 / 3,780 | 11,642 / 3,780 | 379,000 / 379,000 / 12,000 |
| keyword-only | 362,638 / 86,291 | 375,545 / 89,380 | 119,518 / 27,365 | 9,979 / 3,222 | 376,000 / 376,000 / 10,000 |
| keyword-case | 363,103 / 86,363 | 376,010 / 89,452 | 119,902 / 27,452 | 10,363 / 3,292 | 377,000 / 377,000 / 11,000 |
| rest-fn | 362,815 / 86,345 | 375,722 / 89,433 | 119,701 / 27,378 | 24,755 / 6,528 | 376,000 / 376,000 / 25,000 |
| vector-only | 362,670 / 86,309 | 375,577 / 89,398 | 119,699 / 27,433 | 29,333 / 7,539 | 376,000 / 376,000 / 30,000 |
| seq-pipeline | 362,862 / 86,337 | 375,769 / 89,426 | 137,887 / 31,641 | 81,638 / 19,109 | 376,000 / 376,000 / 82,000 |
| keyword-lookups | 363,595 / 86,466 | 376,502 / 89,555 | 130,909 / 30,112 | 98,936 / 23,399 | 377,000 / 377,000 / 99,000 |
| constant-literals | 363,372 / 86,417 | 376,279 / 89,506 | 132,695 / 30,580 | 105,190 / 24,637 | 377,000 / 377,000 / 106,000 |
| constant-keys | 363,588 / 86,405 | 376,495 / 89,494 | 124,716 / 28,830 | 106,963 / 24,982 | 377,000 / 377,000 / 107,000 |
| println-hi | 362,518 / 86,217 | 375,425 / 89,307 | 134,618 / 30,914 | 106,105 / 25,080 | 376,000 / 376,000 / 107,000 |
| vector-printed | 362,543 / 86,227 | 375,450 / 89,316 | 134,830 / 30,991 | 106,317 / 25,153 | 376,000 / 376,000 / 107,000 |
| map-printed | 362,546 / 86,221 | 375,453 / 89,310 | 134,699 / 30,926 | 106,186 / 25,093 | 376,000 / 376,000 / 107,000 |

Small programs at `advanced`, raw bytes and runtime units (out of 1,132):

| Program | Before | After |
| --- | ---: | ---: |
| `(js/console.log :a)` | 119,502 (301) | 9,963 (41) |
| `(js/console.log [1 2 3])` | 119,659 (302) | 29,293 (93) |
| `(js/console.log {})` | 119,528 (301) | 39,501 (127) |
| `(conj [1] 2)` | 119,520 (302) | 33,325 (104) |
| `(assoc {} :a 1)` | 119,459 (301) | 65,654 (190) |
| `(with-meta [1] {:a 1})` | 119,684 (302) | 66,717 (201) |
| `(str 1 2)` | 132,734 (332) | 104,221 (312) |
| `(atom 1)` | 134,196 (338) | 112,428 (351) |

The pellet's targets were 10 KB for the logged keyword, which it meets, and
25 KB for the logged vector and 30 KB for the empty map, which it misses by
4 and 10 KB (light-cljs-230). About 3.6 KB of the vector program is comment
lines, which terse emission removes. Programs without builtin values do not
change. The shared runtime grows by 12,907 bytes (3.6%): the hook tables and
their installs, the native protocol predicates, the moved branches' wrappers
and one unit per protocol. The `none` and `prepare` ceilings rise by 13,000
for that growth.

**Startup and steady state.** `--levels --samples 9 --warmups 1` on master
`75d9dd0` (exported tree, with the new fixtures) and on this change, one
directly after the other; the median one-minute load average was 4.49 and
4.19. Every table is in
[before](results/builtin-types-2026-10-13/before/summary.md) and
[after](results/builtin-types-2026-10-13/after/summary.md), next to their
`samples.csv`. Parse plus first execution at `advanced`, median ms, Node /
QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| keyword-only | 2.276 / 5.790 | 0.541 / 0.379 |
| vector-only | 2.999 / 5.920 | 1.062 / 1.188 |
| rest-fn | 2.116 / 5.882 | 0.566 / 0.953 |
| keyword-case | 2.178 / 5.752 | 0.338 / 0.393 |
| seq-pipeline | 4.876 / 8.015 | 3.229 / 3.968 |
| keyword-lookups | 3.536 / 6.859 | 2.575 / 4.674 |
| constant-keys | 8.320 / 14.048 | 4.606 / 6.343 |
| vector-printed | 3.813 / 6.856 | 3.069 / 5.036 |

Steady state at `advanced`, median µs per call (`none` and `prepare` do not
change beyond the noise):

| Workload | n | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: | ---: |
| seq-pipeline | 200 | 5,353 | 1,076 | 29,039 | 3,350 |
| keyword-lookups | 2,000 | 18,792 | 8,278 | 106,839 | 27,822 |
| constant-literals | 2,000 | 42,176 | 9,620 | 255,713 | 37,344 |
| constant-keys | 200 | 11,169 | 1,042 | 71,685 | 6,441 |
| record-protocol | 2,000 | 9,826 | 9,542 | 16,571 | 16,583 |
| multi-arity | 50,000 | 146 | 145 | 45,347 | 44,925 |

The collection workloads run 2.3 to 11 times faster at `advanced`. Without
protocol implementations every `seq`, `first`, `count`, `get`, `=` and
`hash` skips the protocol lookup (`publicMethod`, `builtin`,
`implementsProtocol` and the protocol slot computation) that each call used
to make before reaching its own branch. `record-protocol`, whose records keep
`installMethods` and so the lookup, does not change, and neither do levels
below `advanced`, which embed the whole runtime.

## Key shapes — October 13, 2026

light-cljs-220 implements tier 4 of "Constant collection literals" in the
[design](../docs/advanced-optimization.md). At `--optimize advanced` a map
literal whose keys are distinct constants and whose values are not is built
from a pooled key shape, `const k3 = rt$mapShape([k0, k1, k2], [<key
hashes>]);`, and its site passes only the values,
`rt$mapFromShape(k3, [k4, l0, l1])`. An evaluation makes the frozen entries
and the map and nothing else: no keyword lookup, no duplicate-key comparison,
no hash closure per entry. Past 8 entries `rt$hashMapShape` builds the hash
trie once and `rt$hashMapFromShape` copies it. Set literals whose first
members (in reader order) are distinct constants add the others to a shape
of those. The new ratchet fixture and `--levels` workload `event-maps` builds
event maps:

```clojure
(defn event [i] {:type :click :x i :y (- 100 i) :button 0 :target "submit"})
(fn [n] (loop [i 0 total 0]
          (if (< i n)
            (let [e (event i)] (recur (inc i) (+ total (:x e) (:y e) (count e))))
            total)))
```

**Allocations.** The fixture in `tests/compiler/key_shapes.rs` counts runtime
calls over 1,000 evaluations of that event map, a 10-entry hash map and
`#{nil i}`: below `Advanced` 16,000 `keyword`, 2,000 `mapFromArray`, 16,000
`putEntry`, 7,000 `entryIndex` and 16,000 `hashNodeAssoc` calls; at
`Advanced` 15 `keyword` and 16 `hashNodeAssoc` calls in all, from the table,
no `mapFromArray`, and 1,001 `putEntry` for the set's other member and its
shape. The 218 allocation fixture's map with a dynamic value goes from 1,001
`mapFromArray` calls to 1 (the pooled constant) and 1,000 `mapFromShape`
calls; its table grows from 13 entries and 583 bytes to 14 and 695.

**Sizes**, bytes of `--compile --optimize advanced --text`; "before" is
master `9e80996` (after light-cljs-227). The constructors join the shared
runtime, which grows by 2,082 bytes at `none` and `prepare` for every
program (`mapShape`, `shapeEntries`, `mapFromShape`, `hashMapShape`, the
trie copy, `hashMapFromShape`, `setShape`, `setFromShape` and their
exports), so every ceiling below `Advanced` rises by 2,000 or 3,000. At
`Advanced` a program that builds a shaped array map embeds `mapShape`,
`shapeEntries` and `mapFromShape` (about 600 bytes with their bindings) and
pays about 70 bytes per shape for its keys and hashes; its sites shrink by
the keys they no longer pass. Since light-cljs-227 keeps only the builders a
program reaches, a program whose map literals are all shaped no longer
embeds `mapFromArray`, `putEntry` and the key search behind them:
`event-maps` loses 14 KB. Programs without shaped literals do not change.

| Fixture | Advanced before | Advanced after | Ceiling before | Ceiling after |
| --- | ---: | ---: | ---: | ---: |
| keyword-lookups | 98,936 | 99,650 | 99,000 | 100,000 |
| constant-literals | 105,190 | 105,940 | 106,000 | 106,000 |
| constant-keys | 106,963 | 107,665 | 107,000 | 108,000 |
| event-maps | 100,606 | 86,592 | — | 87,000 |

`keyword-lookups`, `constant-literals` and `constant-keys` also build
constant maps, which `mapFromArray` builds into the pool, so they keep it
and pay for the shape constructors on top.

**Steady state and startup.** `--levels --samples 9 --warmups 1 --only
event-maps,constant-literals,keyword-lookups,constant-keys,vector-printed`
(the new `--only` option, so that a run takes ten minutes instead of
forty-five) on the exported master tree `ce1fd58` with the new fixture,
workload and option, then on this change, one directly after the other; the
one-minute load average had a median of 4.31 and 4.19. Every table is in
[before](results/key-shapes-2026-10-13/before/summary.md) and
[after](results/key-shapes-2026-10-13/after/summary.md), next to their
`samples.csv`; their sizes predate light-cljs-212 and 227, onto which this
change was then rebased, and which do not change how a map is built.
`event-maps` runs n = 5,000, so a sample's 20 timed calls build 100,000
event maps. Median ms per call:

| Workload | n | Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| event-maps | 5,000 | none | 26.96 | 26.73 | 159.31 | 158.60 |
| event-maps | 5,000 | advanced | 26.33 | 14.77 | 144.50 | 97.66 |
| keyword-lookups | 2,000 | advanced | 18.93 | 14.11 | 107.11 | 99.61 |
| constant-literals | 2,000 | advanced | 42.61 | 36.10 | 256.34 | 238.45 |
| constant-keys | 200 | advanced | 11.17 | 11.10 | 70.88 | 70.48 |

At `Advanced` the event-map workload takes 44% less time in Node and 32% less
in QuickJS, and the workloads whose loops build small maps with dynamic
values (`keyword-lookups`, `constant-literals`' row) 15-25% less in Node and
7% less in QuickJS. `constant-keys` builds its maps once, outside the timed
calls, and does not change; nor does any workload below `Advanced`. In a
Node micro-benchmark (200,000 evaluations of a 10-entry map literal with
two dynamic values) the hash-map shape takes 495 ms instead of 1,470 ms, as
it copies one or two trie nodes instead of inserting ten keys. Parse plus
first execution, median ms:

| Fixture | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: |
| event-maps, none | 8.50 | 8.47 | 23.49 | 23.38 |
| event-maps, advanced | 4.38 | 4.12 | 7.49 | 7.46 |
| constant-literals, advanced | 4.80 | 4.81 | 8.48 | 8.43 |
| keyword-lookups, advanced | 3.56 | 3.60 | 6.96 | 6.86 |
| vector-printed, none | 7.29 | 7.27 | 22.56 | 22.66 |

Startup does not change beyond noise: building a shape at load (and a
hash-map shape's trie) costs about what the first evaluation saves, and the
shared runtime's extra bytes are not resolved at `none`.

## Parameter types — October 13, 2026

light-cljs-229 extends Phase 2 step 3 of the
[design](../docs/advanced-optimization.md) ("Phase 2 parameter type extension
points"): at `--optimize advanced` the parameters of a function whose every
call the program makes itself (a known global that does not escape, a `let`
or `letfn` local only called, or a `fn` called in place) join the arguments
of its calls. So the typed lowerings of light-cljs-210 (`=`, `not`, `count`,
`nth`, record field reads, truth tests) and the devirtualization of
light-cljs-211 now apply inside such functions. Below `Advanced` nothing
changes.

The new ratchet fixture and `--levels` workload `closed-params` reads record
fields, a vector, a string and a keyword through parameters:

```clojure
(defrecord Pt [x y])
(defn dist2 [p q] (let [dx (- (:x p) (:x q)) dy (- (:y p) (:y q))] (+ (* dx dx) (* dy dy))))
(defn at [v i] (nth v i))
(defn width [s] (if (= s "") 0 (count s)))
(defn weight [k] (if (= k :heavy) 10 1))
(defn score [acc v i s k]
  (+ acc (dist2 (->Pt i 0) (->Pt 0 2)) (at v (rem i 3)) (width s) (weight k)))
(fn [n] (loop [i 0 acc 0]
          (if (< i n)
            (recur (inc i) (score acc [1 2 3] i (if (even? i) "" "word") (if (odd? i) :heavy :light)))
            acc)))
```

At `Advanced` its functions become `(l3)["x"] - (l4)["x"]`,
`rt$vectorNth(rt$vectors.get(l7), l8, rt$missing)`, `l9 === ""`,
`(l9).length` and `l10 === k0 || rt$keywordFqn(l10) === "heavy"`.

**Sizes.** Only `closed-params` changes, from 79,842 to 79,800 bytes at
`advanced` (ceiling 80,000): it keeps the generic core for its records and
`rem`, so the lowered calls save their call text but no runtime unit. The
other fixtures' functions escape or take parameters no lowering reads, and the
seven earlier `--levels` workloads compile to the same bytes as before.

**Steady state and startup.** `--levels --samples 9 --warmups 1 --only
closed-params,multi-arity` on this change with parameter types turned off
(a temporary switch that passed inference no closed functions), then with
them, one directly after the other; the one-minute load average had a median
of 6.30 and 3.81. `multi-arity` is the control: its parameters take no
lowering. Both tables are in
[before](results/parameter-types-2026-10-13/before/summary.md) and
[after](results/parameter-types-2026-10-13/after/summary.md), next to their
`samples.csv`. Median ms per call:

| Workload | n | Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| closed-params | 1,000 | none | 13.94 | 14.22 | 45.18 | 45.46 |
| closed-params | 1,000 | advanced | 13.51 | 9.45 | 39.51 | 17.30 |
| multi-arity | 50,000 | advanced | 0.143 | 0.145 | 43.87 | 43.56 |

At `Advanced` the workload takes 30% less time in Node and 56% less in
QuickJS: the field reads no longer go through keyword lookup and the
record's `ILookup`, and `nth`, `count` and `=` no longer dispatch on their
arguments. Parse plus first execution of the fixture at `advanced`, which
runs the loop ten times, drops from 2.79 to 2.11 ms in Node and from 4.16 to
3.82 ms in QuickJS; `multi-arity` and `none` do not change beyond noise.

## Folding, inlining and defines — October 13, 2026

light-cljs-214 implements Phase 2 step 8 of the
[design](../docs/advanced-optimization.md). At `--optimize advanced`,
`src/folding.rs` folds core calls on literals, `if`s on literals, `let`
bindings of literals and `^:const` and `goog-define` reads, and
`src/inlining.rs` replaces calls of single-use and small pure functions with
their bodies, in rounds with the removal of what that leaves unread.
`(goog-define DEBUG false)` and `--define cljs.user/DEBUG=true` give a build
its values at every level.

The new ratchet fixture `debug-logging` gates a logging function behind a
define:

```clojure
(goog-define DEBUG false)
(defn debug-log [& parts] (when DEBUG (js/console.log "[debug]" (pr-str (vec parts)))))
(defn area [w h] (debug-log "area" w h {:w w :h h}) (* w h))
(when DEBUG (debug-log "startup" {:debug true}))
(area (.-length "abc") 4)
```

At `advanced` it is `(() => { return (("abc")["length"] * 4); })()`, byte for
byte the output of `(defn area [w h] (* w h)) (area (.-length "abc") 4)`:
`DEBUG` folds `debug-log` to `nil`, its calls are inlined and dropped with
their arguments, which have no effect, and `area`, called once, is inlined.
Before this change the same program with `(def ^:const DEBUG false)` kept
the logging path, `pr-str` and the printer: 133,451 bytes (31,100 gzip).

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize advanced --text`.
"Before" is master `ec81a9e` (after light-cljs-229). `none` and `prepare` do
not change. Fixtures not listed do not change either: they have no constant
operands and no function a call could take the body of, or only ones whose
body is a `case`, a `recur` target or a function that escapes.

| Fixture | Before | After | Ceilings after |
| --- | ---: | ---: | --- |
| console-log-sum | 192 / 161 | 186 / 157 | 378,000 / 378,000 / 200 |
| add | 32 / 52 | 26 / 46 | 378,000 / 378,000 / 100 |
| raw-js | 271 / 133 | 36 / 54 | 379,000 / 379,000 / 100 |
| multi-arity | 2,040 / 839 | 781 / 308 | 381,000 / 381,000 / 800 |
| constant-literals | 105,940 / 24,913 | 105,878 / 24,890 | 379,000 / 379,000 / 106,000 |
| event-maps | 86,592 / 20,482 | 86,530 / 20,458 | 379,000 / 379,000 / 87,000 |
| closed-params | 79,800 / 19,553 | 79,528 / 19,497 | 381,000 / 381,000 / 80,000 |
| debug-logging | — | 48 / 68 | 379,000 / 379,000 / 100 |

`(+ 1 2)` is `3`. In `raw-js` the call `(add 1 2)` is inlined and folded, so
`add` goes. In `multi-arity` `clamp` and `area` are inlined at their calls,
`(area w w)` inside `area` too, and their results folded: the program is
`(29 + g2_0(10))` with `fib`, a `recur` target, left. `constant-literals`
and `event-maps` inline `row` and `event`, each called once, into the
loop's function, and `closed-params` inlines `at`, `width` and `weight` into
`score`, whose parameter types (light-cljs-229) then apply to them. `debug-logging` at `none` is 378,779 / 90,241 bytes.

A literal that a call built anew at each evaluation, such as `[x 1]` in a
function, stays built at its site after inlining substitutes a constant for
`x`: the pool would otherwise make every evaluation the one shared value.

**Startup and steady state.** `--levels --samples 9 --warmups 1` on this
change, before it was rebased onto light-cljs-227, 220 and 229, and then on an
exported tree of its base, master `75d9dd0`, one directly after the other,
with other builds running: the median one-minute load average was
6.12 (range 3.22–10.14) and 5.48 (2.66–9.26). Every table is in
[before](results/folding-inlining-2026-10-13/before/summary.md) and
[after](results/folding-inlining-2026-10-13/after/summary.md), next to their
`samples.csv`. Parse plus first execution at `advanced`, median ms,
Node / QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| multi-arity | 0.227 / 0.123 | 0.145 / 0.056 |
| raw-js | 0.124 / 0.020 | 0.113 / 0.008 |
| debug-logging | — | 0.114 / 0.011 |

The `multi-arity` workload calls `clamp` with one argument and `area` with
one and with two in a loop (n = 50,000), median µs per call:

| Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: |
| none | 6,602 | 6,695 | 144,175 | 146,820 |
| advanced | 149 | 117 | 44,568 | 42,040 |

Inlining `clamp`'s one-argument method and the two-argument `area` into the
loop makes it 21% faster in Node and 6% in QuickJS; `(area (rem i 7))`
stays a call, since `area`'s one-argument method reads its parameter twice
and the argument is a call. The other fixtures and workloads do not change
beyond the noise.

**Compile time.** Folding and inlining run up to four rounds of linear
passes at `advanced`. Compiling a generated program of 63 definitions (30
pairs of small functions, a constant and a disabled logger; the macro
expansion limits of a single file bound its size) takes 251.5 ms before and
254.5 ms after at `advanced`, and 249.0 and 247.6 ms at `prepare`, median of
15 release-build CLI runs each, interleaved: within the noise of the
process, which macro expansion and the runtime dominate.

## Builtin types, second pass — October 13, 2026

light-cljs-230 finishes rapid type analysis of the runtime's own types
("Phase 2 builtin types, second pass" in the
[design](../docs/advanced-optimization.md)). At `--optimize advanced`:

- core protocol dispatchers and every runtime protocol test answer natively
  where the program has no protocol implementation, so they keep neither
  `builtinCoreImplementation`, `builtin`, `satisfies` nor the protocol
  objects;
- vectors and maps answer their host-facing methods (iteration, `indexOf`,
  `keys`, `entries`, `cnt`, `equiv` of two vectors) from their own items, so
  a program building only vectors and maps keeps no seq;
- the printer reads print options only where a public entry can pass them,
  writes through a native `StringBufferWriter`, and `str` prints only the
  collections a program builds;
- `(atom x)` is `new rt$Atom(x, null, null, null)`, and a map literal of at
  most 8 entries calls `arrayMapFromArray`, which builds no hash map.

The new ratchet fixtures `map-only` and `atom-str` read a map literal and
`str` an atom's value:

```clojure
(js/console.log (.get {:a 1} :a))
(js/console.log (str @(atom 1) 2))
```

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL`. "Before"
is master `c45cd00` (after light-cljs-214). `prepare` equals `none` for
every fixture but `pure-defs`, which is 78 bytes smaller there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 377,620 / 89,894 | 385,908 / 91,800 | 186 / 167 | 186 / 167 | 387,000 / 387,000 / 200 |
| println-hi | 377,507 / 89,823 | 385,795 / 91,729 | 106,105 / 25,090 | 43,926 / 10,951 | 387,000 / 387,000 / 44,000 |
| vector-printed | 377,532 / 89,833 | 385,820 / 91,739 | 106,317 / 25,163 | 51,502 / 12,636 | 387,000 / 387,000 / 52,000 |
| map-printed | 377,535 / 89,827 | 385,823 / 91,733 | 106,186 / 25,103 | 58,863 / 14,369 | 387,000 / 387,000 / 59,000 |
| seq-pipeline | 377,851 / 89,942 | 386,139 / 91,846 | 81,638 / 19,119 | 69,798 / 16,610 | 387,000 / 387,000 / 70,000 |
| add | 377,460 / 89,815 | 385,748 / 91,723 | 26 / 56 | 26 / 56 | 387,000 / 387,000 / 100 |
| rest-fn | 377,804 / 89,949 | 386,092 / 91,854 | 24,755 / 6,538 | 24,795 / 6,580 | 387,000 / 387,000 / 25,000 |
| keyword-case | 378,092 / 89,968 | 386,380 / 91,872 | 10,363 / 3,302 | 10,381 / 3,311 | 388,000 / 388,000 / 11,000 |
| record-protocol | 378,413 / 90,142 | 386,701 / 92,042 | 11,637 / 3,796 | 11,637 / 3,796 | 388,000 / 388,000 / 12,000 |
| pure-defs | 379,049 / 90,251 | 387,337 / 92,144 | 11,471 / 3,725 | 11,471 / 3,725 | 389,000 / 388,000 / 12,000 |
| keyword-lookups | 378,584 / 90,072 | 386,872 / 91,976 | 99,650 / 23,622 | 73,216 / 17,658 | 388,000 / 388,000 / 74,000 |
| constant-literals | 378,361 / 90,023 | 386,649 / 91,927 | 105,878 / 24,900 | 93,957 / 22,456 | 388,000 / 388,000 / 94,000 |
| three-records | 380,550 / 90,440 | 388,838 / 92,325 | 11,642 / 3,790 | 11,642 / 3,790 | 390,000 / 390,000 / 12,000 |
| constant-keys | 378,577 / 90,010 | 386,865 / 91,914 | 107,665 / 25,242 | 96,023 / 22,763 | 388,000 / 388,000 / 97,000 |
| event-maps | 378,109 / 89,985 | 386,397 / 91,887 | 86,530 / 20,468 | 73,800 / 17,792 | 388,000 / 388,000 / 74,000 |
| vector-only | 377,659 / 89,915 | 385,947 / 91,819 | 29,333 / 7,549 | 13,940 / 4,497 | 387,000 / 387,000 / 14,000 |
| keyword-only | 377,627 / 89,897 | 385,915 / 91,802 | 9,979 / 3,232 | 9,997 / 3,241 | 387,000 / 387,000 / 10,000 |
| map-only | 377,665 / 89,905 | 385,953 / 91,809 | 56,368 / 14,055 | 25,808 / 7,292 | 387,000 / 387,000 / 26,000 |
| atom-str | 377,675 / 89,905 | 385,963 / 91,809 | 132,436 / 30,859 | 5,514 / 1,938 | 387,000 / 387,000 / 5,600 |
| raw-js | 378,459 / 90,065 | 386,747 / 91,966 | 36 / 64 | 36 / 64 | 388,000 / 388,000 / 100 |
| multi-arity | 380,288 / 90,327 | 388,576 / 92,220 | 781 / 318 | 781 / 318 | 390,000 / 390,000 / 800 |
| closed-params | 380,023 / 90,478 | 388,311 / 92,366 | 79,528 / 19,507 | 82,559 / 20,346 | 390,000 / 390,000 / 83,000 |
| debug-logging | 378,779 / 90,251 | 387,067 / 92,151 | 48 / 78 | 48 / 78 | 388,000 / 388,000 / 100 |

Small programs at `advanced`, raw bytes and runtime units:

| Program | Before | After |
| --- | ---: | ---: |
| `(js/console.log :a)` | 9,963 (41) | 9,981 (41) |
| `(js/console.log [1 2 3])` | 29,293 (93) | 13,900 (59) |
| `(js/console.log {})` | 39,501 (127) | 22,700 (86) |
| `(js/console.log {:a 1})` | 56,355 (174) | 25,795 (97) |
| `(js/console.log #{1 2})` | 57,854 (179) | 59,923 (188) |
| `(conj [1] 2)` | 33,325 (104) | 35,554 (114) |
| `(fn [& xs] xs)` | 24,755 (77) | 24,795 (80) |
| `(assoc {} :a 1)` | 65,654 (190) | 67,369 (197) |
| `(with-meta [1] {:a 1})` | 66,717 (201) | 54,622 (172) |
| `(fn [x] (str x 1))` | 104,255 (312) | 1,151 (11) |
| `(atom 1)` | 112,428 (351) | 3,993 (20) |
| `(js/console.log (pr-str [1 {:a 2}]))` | 105,160 (315) | 59,308 (187) |
| `(into [] (range 3))` | 116,228 (365) | 63,397 (204) |
| `(reduce + [1 2 3])` | 55,023 (193) | 42,399 (128) |

The vector (25 KB) and map (30 KB) targets are met, and `str` of a scalar
and an atom without options keep about 1 and 4 KB (`(str 1 2)` folds at
compile time since light-cljs-214, so the row calls `str` on a parameter).
Programs that
print keep the printer but no map operations, `installMethods` or protocol
lookup: the printing fixtures drop by 47 to 62 KB. A program that builds
other seqs as well as vectors or maps keeps the native methods and the
generic ones: `(conj [1] 2)`, `(assoc {} :a 1)` and `#{1 2}` grow by about
2 KB. A program with protocol implementations keeps every native protocol
test (`builtinCoreImplementation` is a table of them) and the vectors' and
maps' native methods besides the generic ones it uses: `closed-params`, with
a record, vectors and keyword lookups, grows by 3 KB, and its ceiling rises
from 80,000 to 83,000. Sets keep the generic seqs and both map types (their
`hash-map` field builds a map), which is left for later.

The shared runtime grows by 8,288 bytes (2.2%), 1,906 gzipped: one
native test per core protocol (`builtinCoreImplementation` is now a table of
them), the dispatchers' tests, the printer entries' wrappers and option
hooks, the vectors' and maps' native methods and the transient and `str`
kind hooks. The `none` and `prepare` ceilings rise by 9,000 for that growth;
the `advanced` ceilings drop to the new sizes.

## Local function entries — October 13, 2026

light-cljs-228 finishes Phase 2 step 5 of the
[design](../docs/advanced-optimization.md) ("Phase 2 call shape extension
points"). At `--optimize advanced` a function with several methods or rest
arguments bound by `let` or `letfn`, whose local and name are only called with
counts that select a method, has no `arguments.length` dispatcher: its calls
name the method they run. A method that captures nothing (other than hoisted
methods it calls) becomes a function entry of the constant table, created
once, `k4(a)`; one that reads its scope is a `const` next to the binding,
`l5_1(a, b)`, created at each evaluation as the dispatcher's methods were. A
named global function (`(def f (fn self ...))`) defined at top level calls
its entries through its name, and an escaping function whose `def` may run
again keeps entries with a dispatcher that holds its own evaluation's.

The new ratchet fixture `local-arities` has a named local function whose
one-argument method reads the scope and whose three-argument method does not,
a named local function that captures nothing and a `letfn` that calls both:

```clojure
(defn score [n lo hi]
  (let [clamp (fn clamp ([x] (clamp x lo hi)) ([x l h] (if (< x l) l (if (> x h) h x))))
        area (fn area ([w] (area w w)) ([w h] (* w h)))]
    (letfn [(walk ([i] (walk i 0))
                  ([i acc] (if (< i n) (recur (inc i) (+ acc (clamp i) (area i 2))) acc)))]
      (+ (walk -3) (area 3)))))
(score 12 0 10)
```

At `advanced` `area`'s methods and `clamp`'s three-argument method are table
entries, `clamp`'s one-argument method and `walk`'s methods (which call it)
are `const`s in `score`, and every call is direct, `l14_0(-3)`, `k2(3)`.

**Sizes.** `local-arities` drops from 2,864 / 672 to 1,282 / 417 bytes (raw /
`gzip -9`) at `advanced`; ceilings 389,000 / 389,000 / 1,300 after the
rebase onto light-cljs-214 and 230, which grew the `none` runtime. Every
other fixture compiles to the same bytes as before, at every level.

**Steady state and startup.** `--levels --samples 9 --warmups 1 --only
local-arities,multi-arity` on this change (on top of `ec81a9e`) with local
entries turned off (a temporary switch that gave no local function entries),
then with them, one directly after the other; the one-minute load average
had a median of 2.84 and 3.10. `multi-arity` is the control: its global
functions already had entries. The new `--levels` workload `local-arities`
runs the `multi-arity` loop with `clamp` and `area` as named local functions
of the returned function. Both tables are in
[before](results/local-entries-2026-10-13/before/summary.md) and
[after](results/local-entries-2026-10-13/after/summary.md), next to their
`samples.csv`; both binaries were built on `ec81a9e`, although the after
summary names the rebased commit checked out when the run ended. Median µs
per call (n = 50,000):

| Workload | Level | Node before | Node after | QuickJS before | QuickJS after |
| --- | --- | ---: | ---: | ---: | ---: |
| local-arities | none | 6,727 | 6,743 | 148,460 | 145,570 |
| local-arities | advanced | 3,226 | 108 | 81,741 | 43,939 |
| multi-arity | advanced | 143 | 142 | 43,781 | 43,914 |

At `advanced` the local functions now run as fast as the global ones of
`multi-arity`: 1.9 times faster in QuickJS, and 30 times faster in Node,
which inlines the methods once no call goes through `.call`, the
dispatcher's `switch` and `.apply(this, arguments)`. Parse plus first
execution of the fixture at `advanced` drops from 0.225 to 0.163 ms in Node
and from 0.172 to 0.084 ms in QuickJS; `multi-arity` and `none` do not
change beyond noise.

## Folding against constant collections — October 13, 2026

light-cljs-221 implements tier 5 of the constant collection literals in the
[design](../docs/advanced-optimization.md) ("Constant folding extension
points"). At `--optimize advanced`, `get`, keyword and collection calls,
`get-in`, `nth`, `count`, `empty?`, `contains?`, `seq`, `first`, `rest` and
the collection predicates of constant collections fold to their values, so
destructuring a constant binds its locals statically; a map literal with
constant keys answers its count and the lookups of absent keys whatever its
values, a lookup of a present key reads the key's entry by index
(`rt$maps.get(m)[i].value`), and an `if` on membership in a set of at most
eight keywords is a `switch` on the value's spelling.

The new ratchet fixture `constant-lookups` destructures a `^:const` map and
a vector and tests a keyword against a set:

```clojure
(def ^:const CONFIG {:width 640 :height 480 :title "demo" :modes #{:fast :safe}})
(defn allowed? [mode] (if (#{:fast :safe} mode) 1 0))
(let [{:keys [width height title]} CONFIG
      [x y] [3 4]]
  (+ (* width height) (count title) x y (count (:modes CONFIG))
     (allowed? (keyword (.toLowerCase "FAST")))))
```

At `advanced` it is `(307213 + g1(rt$keywordCore(("FAST")["toLowerCase"]())))`,
with `allowed?` a `switch (rt$keywordFqn(l8))` over `"fast"` and `"safe"`:
no collection, lookup or count is left, and the runtime is the keyword
type.

**Sizes**, raw and `gzip -9` bytes of the `advanced` output. "Before" is
master `27dc281` (after light-cljs-228). `none` and `prepare` do not change,
and neither do the fixtures not listed: they read no constant collection.

| Fixture | Before | After | Ceilings after |
| --- | ---: | ---: | --- |
| constant-literals | 93,957 / 22,446 | 61,383 / 14,794 | 388,000 / 388,000 / 62,000 |
| event-maps | 73,800 / 17,782 | 73,775 / 17,776 | 388,000 / 388,000 / 74,000 |
| constant-lookups | 100,011 / 23,444 | 10,910 / 3,424 | 388,000 / 388,000 / 11,000 |

In `constant-literals` the inlined `row`'s `:pos` and `[:meta :dims]` are
constant values of the map, whose counts fold: each iteration adds `5`, and
the map, `count`, `get-in` and the lookup machinery are gone, leaving
`reduce`, `map` and `range`. In `event-maps` `(:x e)` is the local `i`,
`(count e)` is `5`, and `(:y e)`, a computed value, reads its entry.
`constant-lookups` at `none` is 387,067 / 92,048 bytes.

Two pinned allocation fixtures, `hot_loop_allocations_and_pool_size_are_pinned`
(tests/compiler/constant_pool.rs) and
`event_map_allocations_per_evaluation_are_pinned`
(tests/compiler/key_shapes.rs), now read their maps through `identity`:
read directly, their lookups and counts fold and no map is built.

**Startup and steady state.** `--levels --only
constant-literals,event-maps,constant-lookups --samples 9 --warmups 1` on
an exported tree of its base, master `27dc281` with this change's fixture
set, and then on this change, one directly after the other, with other
builds running: the median one-minute load average was 3.06 (range
2.60–3.64) and 2.67 (2.43–2.98). The tables are in
[before](results/constant-folding-2026-10-13/before/summary.md) and
[after](results/constant-folding-2026-10-13/after/summary.md), next to their
`samples.csv`. Parse plus first execution at `advanced`, median ms, Node /
QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| constant-literals | 2.619 / 4.177 | 1.790 / 2.554 |
| event-maps | 2.203 / 3.101 | 2.011 / 3.118 |
| constant-lookups | 1.812 / 4.083 | 0.326 / 0.389 |

Steady state at `advanced`, median µs per call:

| Workload | n | Node before | Node after | QuickJS before | QuickJS after |
| --- | ---: | ---: | ---: | ---: | ---: |
| constant-literals | 2,000 | 4,898 | 3.3 | 25,612 | 71.4 |
| event-maps | 5,000 | 4,237 | 3,344 | 23,048 | 8,773 |

The `constant-literals` loop body folds to a constant, so a call is the
loop alone. In `event-maps` each iteration still builds the event, but its
lookups no longer search the map: 21% faster in Node and 2.6 times in
QuickJS, whose keyword calls and `entryIndex` scans were costlier.

**Compile time.** Median of 15 interleaved release-build CLI runs at
`advanced`: `benchmarks/fixtures/compiler/collections/value.cljs` takes 17.7
ms before and 18.1 after, and the program of
`lookups_of_constants_fold_to_the_runtime_results`
(tests/compiler/constant_lookups.rs) 19.3 and 18.9 ms: within the noise.
The folder classifies each constant once (memoized) and its key checks are
linear.

## Terse emission — October 13, 2026

light-cljs-215 implements Phase 3 of the
[design](../docs/advanced-optimization.md) ("Phase 3 terse emission
extension points"). At `--optimize advanced` the output is terse:

- `build.rs` writes every runtime unit without comments or whitespace, with
  its locals renamed to short names per scope, and the slice names its
  top-level bindings by frequency; runtime constructors keep their names;
- every name the emitter binds gets a short name by frequency, member reads
  with an identifier key are dotted, redundant parentheses and semicolons go,
  adjacent `let` and `const` statements join, and the tokens join without
  whitespace;
- `--pseudo-names` keeps the readable output.

`none` and `prepare` output does not change, nor does the shared runtime.

**Sizes**, raw and `gzip -9` bytes (of stdin, so without a file name) of
`--compile --optimize advanced`. "Before" is master `f0b800c` (after
light-cljs-221). "Stripped" and "+ runtime locals" are this change with the
build's renaming steps turned off: the program rewrite and the runtime's
comment and whitespace stripping only, and then with runtime locals renamed
too (their slices' top-level names are inconsistent, so they are sizes, not
runnable programs).

| Fixture | Before | Stripped | + runtime locals | After | Advanced ceiling |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 186 / 157 | 151 / 140 | 151 / 140 | 151 / 140 | 200 → 200 |
| println-hi | 43,926 / 10,941 | 32,620 / 8,108 | 26,774 / 7,382 | 21,456 / 6,737 | 44,000 → 22,000 |
| vector-printed | 51,502 / 12,626 | 38,207 / 9,377 | 31,322 / 8,557 | 24,932 / 7,794 | 52,000 → 25,000 |
| map-printed | 58,863 / 14,359 | 43,245 / 10,467 | 35,483 / 9,513 | 28,170 / 8,654 | 59,000 → 29,000 |
| seq-pipeline | 69,798 / 16,600 | 51,853 / 12,328 | 42,312 / 11,148 | 33,742 / 10,138 | 70,000 → 34,000 |
| add | 26 / 46 | 19 / 39 | 19 / 39 | 19 / 39 | 100 → 100 |
| rest-fn | 24,795 / 6,570 | 17,641 / 4,609 | 14,395 / 4,169 | 11,423 / 3,709 | 25,000 → 12,000 |
| keyword-case | 10,381 / 3,301 | 6,673 / 2,112 | 5,483 / 1,879 | 4,343 / 1,647 | 11,000 → 4,400 |
| record-protocol | 11,637 / 3,786 | 8,689 / 2,941 | 6,819 / 2,627 | 5,750 / 2,423 | 12,000 → 5,800 |
| pure-defs | 11,471 / 3,715 | 8,579 / 2,887 | 6,709 / 2,574 | 5,640 / 2,372 | 12,000 → 5,700 |
| keyword-lookups | 73,216 / 17,648 | 53,081 / 12,671 | 42,950 / 11,414 | 34,465 / 10,409 | 74,000 → 35,000 |
| constant-literals | 61,383 / 14,794 | 45,412 / 10,879 | 36,835 / 9,819 | 29,502 / 8,968 | 62,000 → 30,000 |
| constant-lookups | 10,910 / 3,424 | 7,245 / 2,261 | 5,903 / 2,013 | 4,723 / 1,775 | 11,000 → 4,800 |
| three-records | 11,642 / 3,780 | 8,696 / 2,941 | 6,826 / 2,626 | 5,757 / 2,422 | 12,000 → 5,800 |
| constant-keys | 96,023 / 22,753 | 70,353 / 16,716 | 56,538 / 14,949 | 45,095 / 13,539 | 97,000 → 46,000 |
| event-maps | 73,775 / 17,776 | 53,870 / 12,895 | 43,688 / 11,612 | 35,048 / 10,567 | 74,000 → 36,000 |
| vector-only | 13,940 / 4,487 | 9,308 / 2,965 | 7,708 / 2,672 | 5,883 / 2,321 | 14,000 → 5,900 |
| keyword-only | 9,997 / 3,231 | 6,576 / 2,081 | 5,394 / 1,853 | 4,270 / 1,617 | 10,000 → 4,300 |
| map-only | 25,808 / 7,282 | 17,438 / 4,826 | 14,164 / 4,308 | 11,068 / 3,820 | 26,000 → 12,000 |
| atom-str | 5,514 / 1,928 | 3,557 / 1,185 | 2,929 / 1,059 | 2,284 / 904 | 5,600 → 2,300 |
| raw-js | 36 / 54 | 36 / 54 | 36 / 54 | 36 / 54 | 100 → 100 |
| multi-arity | 781 / 308 | 318 / 212 | 318 / 212 | 318 / 212 | 800 → 400 |
| closed-params | 82,559 / 20,336 | 60,718 / 15,165 | 49,653 / 13,742 | 37,754 / 12,276 | 83,000 → 38,000 |
| debug-logging | 48 / 68 | 31 / 51 | 31 / 51 | 31 / 51 | 100 → 100 |
| local-arities | 1,282 / 417 | 490 / 278 | 490 / 278 | 490 / 278 | 1,300 → 500 |
| total | 749,499 / 190,387 | 544,806 / 138,188 | 442,930 / 124,690 | 352,350 / 112,866 | |

Over all fixtures the output is 53% smaller raw and 41% smaller gzipped.
Stripping alone takes 27% of the bytes away, renaming the runtime's locals
another 14 points raw (7 gzipped) and naming the slice's top-level bindings
12 (6): renaming inside runtime units is worth its build-time pass. About
4.6 KB of the 13.9 KB `[1 2 3]` program were the runtime's comments and
whitespace; it is now 5,883 bytes, 2,321 gzipped. Keeping the 35 constructor
names (`KeywordValue`, `PersistentVectorValue`, ...) costs 2.6% raw and 0.9%
gzipped; all other runtime bindings are renamed. Programs with raw `js*`
text (`raw-js`) keep their layout and a prefixed short name for each
compiler name. The ABI property names are not mangled: they are at most 568
bytes of any fixture (`closed-params`), see light-cljs-232.

**Startup and steady state.** `--levels --only
constant-literals,event-maps,constant-lookups,vector-only --samples 9
--warmups 1`, with other builds running: the median one-minute load average
was 3.02 (range 2.37–4.56). The table is in
[after](results/terse-emission-2026-10-13/after/summary.md), next to its
`samples.csv`; "before" is the [after run of
light-cljs-221](results/constant-folding-2026-10-13/after/summary.md), whose
outputs are this change's base. Parse plus first execution at `advanced`,
median ms, Node / QuickJS:

| Fixture | Before | After |
| --- | ---: | ---: |
| constant-literals | 1.790 / 2.554 | 1.554 / 2.325 |
| event-maps | 2.011 / 3.118 | 1.794 / 2.766 |
| constant-lookups | 0.326 / 0.389 | 0.284 / 0.328 |
| vector-only | – | 0.553 / 0.470 |

Node parses the terse output 15–20% faster and QuickJS evaluates it 9–16%
faster. Steady state does not change beyond noise (median µs per call,
Node / QuickJS): `constant-literals` 3.3 / 71.4 before and 3.2 / 67.4 after,
`event-maps` 3,344 / 8,773 and 3,489 / 8,730.

**Compile time.** Median of 15 interleaved release-build CLI runs at
`advanced`, terse and with `--pseudo-names`:
`benchmarks/fixtures/compiler/collections/value.cljs` 14.1 and 13.5 ms,
`closed-params` 15.1 and 14.6 ms, `vector-only` 13.3 and 13.2 ms. The build
script writes the terse runtime (470 KB of Rust source) in about 0.1 s.

## Builtin types, sets — October 13, 2026

light-cljs-231 gives sets what light-cljs-230 gave vectors and maps ("Phase
2 builtin types, sets" in the [design](../docs/advanced-optimization.md)).
At `--optimize advanced`:

- a set answers iteration, `keys`, `values`, `entries` and `has` from its
  members, and compares two sets without `count` or `contains`, where the
  program has no protocol implementation;
- a set has the hash-map kind only where `putMember` promoted it or the
  public constructor (`publicHashSet`, kept where the program names
  `PersistentHashSet`) gave it, so the hash hooks belong to those two
  instead of the set type, and its `hash-map` field is an array map for an
  array set;
- a set literal of at most 8 constants calls `arraySetFromArray`;
- membership in a set literal of at most 8 keywords, or of strings and
  numbers, compares the value with each member in value position too, and
  an `if` on membership in a string or number set is a `switch`.

The new ratchet fixture `set-only` searches and iterates a set literal:

```clojure
(js/console.log (.has #{1 2 3} 2) (.join (js/Array.from #{1 2 3}) " "))
```

**Sizes**, raw and `gzip -9` bytes (of stdin) of `--compile --optimize
LEVEL`. "Before" is master `58f8c05` (after light-cljs-215). `prepare`
equals `none` for every fixture but `pure-defs`, which is 78 bytes smaller
there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 385,908 / 91,790 | 388,296 / 92,339 | 151 / 140 | 151 / 140 | 389,000 / 389,000 / 200 |
| println-hi | 385,795 / 91,719 | 388,183 / 92,268 | 21,456 / 6,737 | 21,456 / 6,737 | 389,000 / 389,000 / 22,000 |
| vector-printed | 385,820 / 91,729 | 388,208 / 92,278 | 24,932 / 7,794 | 24,932 / 7,794 | 389,000 / 389,000 / 25,000 |
| map-printed | 385,823 / 91,723 | 388,211 / 92,271 | 28,170 / 8,654 | 28,179 / 8,662 | 389,000 / 389,000 / 29,000 |
| seq-pipeline | 386,139 / 91,836 | 388,527 / 92,388 | 33,742 / 10,138 | 33,742 / 10,138 | 389,000 / 389,000 / 34,000 |
| add | 385,748 / 91,713 | 388,136 / 92,260 | 19 / 39 | 19 / 39 | 389,000 / 389,000 / 100 |
| rest-fn | 386,092 / 91,844 | 388,480 / 92,395 | 11,423 / 3,709 | 11,423 / 3,709 | 389,000 / 389,000 / 12,000 |
| keyword-case | 386,380 / 91,862 | 388,768 / 92,417 | 4,343 / 1,647 | 4,343 / 1,647 | 389,000 / 389,000 / 4,400 |
| record-protocol | 386,701 / 92,032 | 389,089 / 92,586 | 5,750 / 2,423 | 5,750 / 2,423 | 390,000 / 390,000 / 5,800 |
| pure-defs | 387,337 / 92,134 | 389,725 / 92,692 | 5,640 / 2,372 | 5,640 / 2,372 | 390,000 / 390,000 / 5,700 |
| keyword-lookups | 386,872 / 91,966 | 389,260 / 92,521 | 34,465 / 10,409 | 34,474 / 10,416 | 390,000 / 390,000 / 35,000 |
| constant-literals | 386,649 / 91,917 | 389,037 / 92,471 | 29,502 / 8,968 | 29,502 / 8,968 | 390,000 / 390,000 / 30,000 |
| constant-lookups | 387,067 / 92,048 | 389,455 / 92,606 | 4,723 / 1,775 | 4,723 / 1,775 | 390,000 / 390,000 / 4,800 |
| three-records | 388,838 / 92,315 | 391,226 / 92,876 | 5,757 / 2,422 | 5,757 / 2,422 | 392,000 / 392,000 / 5,800 |
| constant-keys | 386,865 / 91,904 | 389,253 / 92,458 | 45,095 / 13,539 | 45,280 / 13,599 | 390,000 / 390,000 / 46,000 |
| event-maps | 386,397 / 91,877 | 388,785 / 92,431 | 35,048 / 10,567 | 35,057 / 10,573 | 389,000 / 389,000 / 36,000 |
| vector-only | 385,947 / 91,809 | 388,335 / 92,358 | 5,883 / 2,321 | 5,883 / 2,321 | 389,000 / 389,000 / 5,900 |
| keyword-only | 385,915 / 91,792 | 388,303 / 92,341 | 4,270 / 1,617 | 4,270 / 1,617 | 389,000 / 389,000 / 4,300 |
| map-only | 385,953 / 91,799 | 388,341 / 92,349 | 11,068 / 3,820 | 11,077 / 3,829 | 389,000 / 389,000 / 12,000 |
| atom-str | 385,963 / 91,799 | 388,351 / 92,349 | 2,284 / 904 | 2,284 / 904 | 389,000 / 389,000 / 2,300 |
| set-only | 385,984 / 91,819 | 388,372 / 92,369 | 27,704 / 8,537 | 11,961 / 3,934 | 389,000 / 389,000 / 12,000 |
| raw-js | 386,747 / 91,956 | 389,135 / 92,510 | 36 / 54 | 36 / 54 | 390,000 / 390,000 / 100 |
| multi-arity | 388,576 / 92,210 | 390,964 / 92,771 | 318 / 212 | 318 / 212 | 391,000 / 391,000 / 400 |
| closed-params | 388,311 / 92,356 | 390,699 / 92,913 | 37,754 / 12,276 | 37,763 / 12,282 | 391,000 / 391,000 / 38,000 |
| debug-logging | 387,067 / 92,141 | 389,455 / 92,697 | 31 / 51 | 31 / 51 | 390,000 / 390,000 / 100 |
| local-arities | 388,999 / 92,301 | 391,387 / 92,865 | 490 / 278 | 490 / 278 | 392,000 / 392,000 / 500 |

Small programs at `advanced`, raw and gzip bytes (the membership rows
export `f` and call it once):

| Program | Before | After |
| --- | ---: | ---: |
| `(js/console.log #{1 2})` | 27,667 / 8,512 | 11,924 / 3,910 |
| `(js/console.log {:a 1})` | 11,061 / 3,817 | 11,070 / 3,826 |
| `(js/console.log #{1 2 3 4 5 6 7 8 9})` | 27,680 / 8,525 | 28,011 / 8,586 |
| `(js/console.log (conj #{1} 2))` | 34,457 / 10,450 | 34,780 / 10,503 |
| `(js/console.log (conj [1] 2))` | 16,867 / 5,280 | 16,867 / 5,280 |
| `(defn f [k] (if (contains? #{:fast :safe} k) 1 0))` | 4,444 / 1,690 | 4,444 / 1,690 |
| `(defn f [k] (contains? #{:fast :safe} k))` | 28,666 / 8,796 | 4,412 / 1,679 |
| `(defn f [k] (#{:fast :safe} k))` | 28,670 / 8,792 | 4,437 / 1,689 |
| `(defn f [n] (contains? #{1 2} n))` | 27,755 / 8,547 | 241 / 181 |
| `(defn f [s] (contains? #{"fast" "safe"} s))` | 27,769 / 8,553 | 256 / 186 |

`#{1 2}` now keeps 104 runtime units instead of 188: those of `{:a 1}` but
its keyword's, which the array map of the `hash-map` field brings (about 5
KB), and 14 of the set's own; no seq, hash map or hashing. A membership
test in value position keeps no set at all. A program that builds larger
or dynamic sets keeps the native methods besides the generic ones and
hashing: `#{1 ... 9}`, `(conj #{1} 2)` and `constant-keys` grow by 185 to
331 bytes. Programs that keep `contains` and equality, every map program
among them, gain the 9-byte install of `hostHooks.has`. The vector and map
programs that keep both native and generic paths (`(conj [1] 2)`,
`closed-params`) do not change.

The shared runtime grows by 2,388 bytes (0.6%), 555 gzipped: the sets'
native methods, the public constructor split from the set type, the
array-set builder and the new hooks. The `none` and `prepare` ceilings
rise by 1,000 to 3,000 for that growth (each to its new size rounded up to
the next 1,000); the `advanced` ceilings do not change, and `set-only`'s is
12,000.

## Collection throughput against upstream — October 13, 2026

light-cljs-233 extends `--levels` ("Startup and steady state by optimization
level" above) for the runtime-performance pellets to measure against. Every
steady-state workload is now also compiled by the pinned ClojureScript
checkout and timed next to light-cljs. Eight new collection workloads, each at
n = 2,000 and n = 20,000, measure the operations that are linear per element
today:

| Workload | `(f n)` |
| --- | --- |
| vector-conj | `(reduce conj [] (range n))`, then its count plus its last element |
| map-assoc | `(reduce (fn [m i] (assoc m i i)) {} (range n))`, likewise |
| set-conj | `(reduce conj #{} (range n))`, likewise |
| vector-nth | `nth` at each index of a built `(vec (range n))`, summed |
| map-get | `get` of each key of a built n-entry map, summed |
| map-assoc-existing | 1,000 `assoc`s of a new key, each onto the same built n-entry map; the counts summed |
| subvec-reduce | `(reduce + 0 (subvec v 1 n))` over a built vector |
| vector-into | `(into [] (range n))`, which goes through a transient |

A built collection is made by the first call, which is untimed, and kept in a
volatile; the map is built by `into` with a transducer, which both compilers
do with a transient. From n = 2,000 to 20,000, n constant-time operations
take ten times as long and n linear-time ones a hundred times;
`map-assoc-existing` makes 1,000 operations at both sizes, so with a
logarithmic `assoc` its time barely grows.

The pellet's acceptance command, the eight collection workloads with upstream,
took 194 s (3 min 14 s) on the shared machine:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only vector-conj,map-assoc,set-conj,vector-nth,map-get,map-assoc-existing,subvec-reduce,vector-into \
  --output DIR
```

Method, where it differs from the section above:

- **Upstream steady state.** Each workload becomes namespace
  `bench.steady.NAME`, whose last form is stored with `(aset js/globalThis
  "levelsSteady" ...)`. It is compiled at `:optimizations :advanced` before
  the rounds, in the same JVM as the size column
  ([support/upstream_sizes.clj](support/upstream_sizes.clj) now takes the
  variants per program). The Node driver calls `globalThis.levelsSteady` when
  the program sets it and checks the result against the value Rust computes,
  as for light-cljs. Each round samples the upstream build next to the
  light-cljs builds of the same workload, rotating their order. `summary.md`
  has the two medians and their ratio under "Upstream ClojureScript". A
  compilation failure leaves the light-cljs timings in place and fails the run
  at the end.
- **Collection workloads** run in Node at `advanced` only, the build compared
  with upstream: at n = 20,000 a QuickJS call to map-assoc would take about a
  minute. The nine ratchet workloads still run at every level in both engines.
- **Call budget.** A sample makes up to 10 untimed and then up to 20 timed
  calls, as before, but each phase stops early when one more call, at its
  average so far, would take it past 0.5 s; every phase makes at least one
  call. Calls under 25 ms are unaffected. Longer ones are averaged over fewer
  calls: QuickJS `multi-arity` at `none` (147 ms) over 3, Node
  `constant-literals` at `none` (48 ms) over 9 or 10. Their medians match the
  earlier runs within the noise (QuickJS `multi-arity` at `none`, 147 ms
  against 144 ms in "Local function entries"). `samples.csv` now has each
  sample's n and timed calls.
- **Slow targets.** A target (workload, n, engine and build) whose call takes
  longer than the budget is sampled in 3 rounds only, so that a 5-second call
  costs 30 seconds instead of minutes. Today that is vector-conj, map-assoc,
  set-conj and map-assoc-existing at n = 20,000 in light-cljs; their medians
  are of 3 samples.
- **Warm-up rounds** run only the startup fixtures. Each steady-state sample is
  a fresh process or session that warms itself with its untimed calls, so a
  warm-up round added nothing but time.

Baseline: this change on master `49e64cc`. Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM,
24 CPUs; 15 timed rounds. Another worker was building and testing on the
machine: the one-minute load average was 2.4–11.2 (median 3.7) over the
rounds, so single numbers can be off by 10%, and runs of the same commit
differed by up to that much. The ratios are large enough not to depend on it.
[summary.md](results/collections-2026-10-13/summary.md) has every measure with
its minimum (including the startup and size tables of the nine ratchet
fixtures), and [samples.csv](results/collections-2026-10-13/samples.csv) all
3,402 samples. Its command was the one above with the nine ratchet workloads
added to `--only`.

Collection workloads, Node, median ms per call. Growth is the time at n =
20,000 over the time at n = 2,000, light-cljs / upstream:

| Workload | n | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream | Growth |
| --- | ---: | ---: | ---: | ---: | ---: |
| vector-conj | 2,000 | 5.63 | 0.092 | 61× | |
| vector-conj | 20,000 | 717 | 0.483 | 1,486× | 127× / 5.3× |
| map-assoc | 2,000 | 56.4 | 0.294 | 192× | |
| map-assoc | 20,000 | 5,643 | 3.38 | 1,670× | 100× / 11.5× |
| set-conj | 2,000 | 55.8 | 0.314 | 178× | |
| set-conj | 20,000 | 5,730 | 3.50 | 1,639× | 103× / 11.1× |
| vector-nth | 2,000 | 0.082 | 0.018 | 4.5× | |
| vector-nth | 20,000 | 0.566 | 0.124 | 4.6× | 6.9× / 6.9× |
| map-get | 2,000 | 0.629 | 0.053 | 12× | |
| map-get | 20,000 | 36.9 | 0.545 | 68× | 59× / 10.3× |
| map-assoc-existing | 2,000 | 50.8 | 0.117 | 435× | |
| map-assoc-existing | 20,000 | 608 | 0.136 | 4,460× | 12× / 1.2× |
| subvec-reduce | 2,000 | 0.162 | 0.019 | 8.7× | |
| subvec-reduce | 20,000 | 1.54 | 0.079 | 19× | 9.5× / 4.2× |
| vector-into | 2,000 | 1.93 | 0.029 | 68× | |
| vector-into | 20,000 | 19.8 | 0.195 | 101× | 10× / 6.7× |

The ratchet workloads, same run (n as in the section above):

| Workload | n | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream |
| --- | ---: | ---: | ---: | ---: |
| seq-pipeline | 200 | 1.09 | 0.074 | 15× |
| record-protocol | 2,000 | 9.83 | 0.022 | 452× |
| keyword-lookups | 2,000 | 3.70 | 0.165 | 22× |
| constant-literals | 2,000 | 0.003 | 0.220 | 0.02× |
| constant-keys | 200 | 1.11 | 0.290 | 3.8× |
| multi-arity | 50,000 | 0.119 | 0.111 | 1.1× |
| event-maps | 5,000 | 3.85 | 0.128 | 30× |
| closed-params | 1,000 | 9.52 | 0.077 | 123× |
| local-arities | 50,000 | 0.109 | 0.375 | 0.29× |

What the baseline shows:

- **Quadratic builds.** Building a vector, map or set one element at a time
  grows 100–127× for tenfold n in light-cljs and 5–12× upstream, so the gap
  widens from 61–192× at n = 2,000 to about 1,500× at n = 20,000: 0.7 s for
  a 20,000-element vector and 5.6 s for a map or set, against 0.5 ms and 3.4
  ms. Every `conj` copies the vector, and every `assoc` or `conj` copies and
  searches the linear entry array (the group context's causes). vector-conj
  at 20,000 is the number light-cljs-234 must bring within 3× of upstream,
  map-assoc and map-get light-cljs-235's, set-conj light-cljs-236's.
- **Updating a large map** costs light-cljs 0.05 ms per `assoc` on a
  2,000-entry map and 0.6 ms on a 20,000-entry one, linear in the size;
  upstream's 0.12–0.14 µs barely moves. That is the largest ratio, 4,460×.
- **Lookups.** `get` on a built hash map grows 59× for tenfold n (upstream
  10×), so it is far from constant; `nth` on a vector is within 5× of
  upstream and grows with n, as upstream's does.
- **Transients and views.** `into` through a transient is linear (10×), 70–100×
  slower than upstream per element; `subvec` then `reduce` is linear and
  9–19× slower.
- **Ratchet workloads.** Where advanced output folds the work away
  (`constant-literals`) or calls local arities directly (`local-arities`,
  `multi-arity`), light-cljs matches or beats upstream. Where a call reaches
  the runtime's sequences or dispatch it is 15–30× slower (`seq-pipeline`,
  `keyword-lookups`, `event-maps`), lookups with collection keys 3.8×
  (`constant-keys`), and record construction with a protocol
  call is 120–450× slower (`closed-params`, `record-protocol`); upstream's
  `record-protocol` takes 11 ns per iteration, so Closure and V8 have likely
  removed the record allocation. `seq-pipeline` and `record-protocol` are the
  numbers light-cljs-237 must bring within 2× of upstream.

## Seq operations and the printer by kind — October 13, 2026

light-cljs-238 slices the printer and the generic seq operations by the
kinds a program builds ("Phase 2 builtin types, seq operations and the
printer" in the [design](../docs/advanced-optimization.md)). At
`--optimize advanced`:

- the printer prints host values itself and other values through a chain
  of the printers of the kinds the program builds, each added by the kind's
  builders; without protocol implementations it walks arrays, vectors, maps
  and sets, and `pr-str` its arguments, by index rather than through seqs;
- `seq`, `first`, `rest`, `next`, `count`, `nth` and `reduce` look up
  protocol implementations only through a wrapper that `installMethods`
  installs;
- the seq operations' branches for vectors and public array maps, and
  `reduce`'s for `iterate` and `cycle`, are kind hooks, so a program that
  builds no vector keeps none for them;
- the host clock and random source are kept only with a reader;
- the runtime's types have their upstream fields and methods (`first`,
  `cnt`, `meta`, `sval`, ...) only where the program names one, or calls
  `reset-meta!` or `alter-meta!`; their host-facing methods (`equiv`,
  `indexOf`, `lastIndexOf`, iteration) stay;
- `reduce` takes a `reduced` value apart without `deref` where no protocol
  implementation exists, and the printer fallback for messages is kept
  apart from the collections' `toString` fallback.

The new ratchet fixture `mapv-count` is the pellet's `mapv` program:

```clojure
(js/console.log (count (mapv inc [1 2 3])))
```

**Sizes**, raw and `gzip -9` bytes (of stdin) of `--compile --optimize
LEVEL`, on the 24-CPU Mac (sizes do not depend on load; load average 10.2
from peer builds). "Before" is master `49e64cc`. `prepare` equals `none`
for every fixture but `pure-defs`, which is 78 bytes smaller there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 388,296 / 92,339 | 395,290 / 94,192 | 151 / 140 | 151 / 140 | 396,000 / 396,000 / 200 |
| println-hi | 388,183 / 92,268 | 395,177 / 94,120 | 21,456 / 6,737 | 4,771 / 2,021 | 396,000 / 396,000 / 4,800 |
| vector-printed | 388,208 / 92,278 | 395,202 / 94,129 | 24,932 / 7,794 | 9,703 / 3,740 | 396,000 / 396,000 / 9,800 |
| map-printed | 388,211 / 92,271 | 395,205 / 94,123 | 28,179 / 8,662 | 13,514 / 4,825 | 396,000 / 396,000 / 14,000 |
| seq-pipeline | 388,527 / 92,388 | 395,521 / 94,242 | 33,742 / 10,138 | 24,450 / 7,610 | 396,000 / 396,000 / 25,000 |
| add | 388,136 / 92,260 | 395,130 / 94,110 | 19 / 39 | 19 / 39 | 396,000 / 396,000 / 100 |
| rest-fn | 388,480 / 92,395 | 395,474 / 94,249 | 11,423 / 3,709 | 9,465 / 3,177 | 396,000 / 396,000 / 9,500 |
| keyword-case | 388,768 / 92,417 | 395,762 / 94,272 | 4,343 / 1,647 | 3,428 / 1,324 | 396,000 / 396,000 / 3,500 |
| record-protocol | 389,089 / 92,586 | 396,083 / 94,442 | 5,750 / 2,423 | 5,750 / 2,423 | 397,000 / 397,000 / 5,800 |
| pure-defs | 389,725 / 92,692 | 396,719 / 94,555 | 5,640 / 2,372 | 5,640 / 2,372 | 397,000 / 397,000 / 5,700 |
| keyword-lookups | 389,260 / 92,521 | 396,254 / 94,379 | 34,474 / 10,416 | 29,113 / 9,056 | 397,000 / 397,000 / 30,000 |
| constant-literals | 389,037 / 92,471 | 396,031 / 94,327 | 29,502 / 8,968 | 20,612 / 6,561 | 397,000 / 397,000 / 21,000 |
| constant-lookups | 389,455 / 92,606 | 396,449 / 94,470 | 4,723 / 1,775 | 3,808 / 1,453 | 397,000 / 397,000 / 3,900 |
| three-records | 391,226 / 92,876 | 398,220 / 94,742 | 5,757 / 2,422 | 5,757 / 2,422 | 399,000 / 399,000 / 5,800 |
| constant-keys | 389,253 / 92,458 | 396,247 / 94,317 | 45,280 / 13,599 | 39,897 / 12,202 | 397,000 / 397,000 / 40,000 |
| event-maps | 388,785 / 92,431 | 395,779 / 94,286 | 35,057 / 10,573 | 29,341 / 9,123 | 396,000 / 396,000 / 30,000 |
| vector-only | 388,335 / 92,358 | 395,329 / 94,213 | 5,883 / 2,321 | 4,224 / 1,739 | 396,000 / 396,000 / 4,300 |
| keyword-only | 388,303 / 92,341 | 395,297 / 94,195 | 4,270 / 1,617 | 3,355 / 1,297 | 396,000 / 396,000 / 3,400 |
| map-only | 388,341 / 92,349 | 395,335 / 94,204 | 11,077 / 3,829 | 9,146 / 3,194 | 396,000 / 396,000 / 9,200 |
| atom-str | 388,351 / 92,349 | 395,345 / 94,202 | 2,284 / 904 | 2,284 / 904 | 396,000 / 396,000 / 2,300 |
| set-only | 388,372 / 92,369 | 395,366 / 94,224 | 11,961 / 3,934 | 5,085 / 1,949 | 396,000 / 396,000 / 5,100 |
| mapv-count | - | 395,357 / 94,213 | - | 28,725 / 8,914 | 396,000 / 396,000 / 29,000 |
| raw-js | 389,135 / 92,510 | 396,129 / 94,372 | 36 / 54 | 36 / 54 | 397,000 / 397,000 / 100 |
| multi-arity | 390,964 / 92,771 | 397,958 / 94,638 | 318 / 212 | 318 / 212 | 398,000 / 398,000 / 400 |
| closed-params | 390,699 / 92,913 | 397,693 / 94,782 | 37,763 / 12,282 | 35,923 / 11,755 | 398,000 / 398,000 / 36,000 |
| debug-logging | 389,455 / 92,697 | 396,449 / 94,557 | 31 / 51 | 31 / 51 | 397,000 / 397,000 / 100 |
| local-arities | 391,387 / 92,865 | 398,381 / 94,738 | 490 / 278 | 490 / 278 | 399,000 / 399,000 / 500 |

All four of the design's targets are met: `println-hi` (4,771, target
5,000), `vector-printed` (9,703, target 15,000), `map-printed` (13,514,
target 20,000) and `seq-pipeline` (24,450, target 25,000). The
upstream fields alone were about 3.7 KB of `seq-pipeline`: the getters of
its seq types, `publicFields` and the stores `publicFields` tests to
tell a runtime value from a host one.

Other programs at `advanced`, raw and gzip bytes:

| Program | Before | After |
| --- | ---: | ---: |
| `(js/console.log (count (mapv inc [1 2 3])))` | 33,935 / 10,231 | 28,725 / 8,914 |
| `(enable-console-print!) (defmulti area :shape) (defmethod area :square [{:keys [side]}] (* side side)) (println (area {:shape :square :side 3}))` | 57,807 / 17,303 | 52,739 / 16,068 |
| `(js/console.log (conj [1] 2))` | 16,867 / 5,280 | 13,759 / 4,433 |

`println-hi` keeps 31 runtime units instead of 139: the printer's host
branches, its string writer and the print functions, no seq, keyword,
symbol, collection type or metadata. A program with protocol
implementations keeps the operations' protocol wrappers (about 200 bytes),
but no longer the upstream fields of the types it builds: `closed-params`
drops by 1,840 bytes. Every fixture that builds a runtime value without
naming an upstream member shrinks; none grows.

The shared runtime grows by 6,994 bytes (1.8%), 1,853 gzipped: the kind
printers and their chain, the walks without seqs, the protocol wrappers,
the new kind hooks, the split of each type's members into `.members` and
`.fields`, and their comments. The `none` and `prepare` ceilings rise by
7,000 for that growth (each to its new size rounded up to the next 1,000);
the `advanced` ceilings of every fixture that shrank drop.

## String table — October 13, 2026

light-cljs-242 measures Phase 3's last item in the
[design](../docs/advanced-optimization.md), a string table for repeated
long strings "when it pays after gzip". It does not pay: hoisting the
repeated string literals of terse output into one `const` saves about 1%
raw and makes every fixture that has one larger gzipped, so nothing is
built.

**Method.** The `--compile --optimize advanced` output of each ratchet
fixture (27 now: `set-only` and `mapv-count` joined the 25 since
light-cljs-232) and of `(read-string "{:a 1}")`, on master `50eeded`
(after light-cljs-238 and light-cljs-240; light-cljs-237, which takes
the protocol-key strings out of dispatch, had not landed). A throwaway
babashka script lexed each output as `src/js_syntax.rs` does (strings,
templates, regular expressions) and replaced every string literal that
occurs at least twice, except the `"use strict"` directive, by a name
declared once at the top of the program, `(()=>{const gA="array-map",...;`.
Names are the shortest identifiers the output does not use, the first to
the strings with the most bytes: one letter in small outputs, two in large
ones. An object key becomes `[gA]:` and a name next to an identifier gets a
space (`case gA:`). Every rewritten program runs under Node with the same
output as the original. Three tables:

- *All*: every repeated string;
- *Paying*: only the strings whose raw saving is positive (their uses
  minus the names and the declaration);
- *Greedy*: the paying strings added one at a time, largest first, each
  kept only if it lowers the gzipped size: a greedy search guided by gzip,
  not an exhaustive one.

**Sizes** of the output, and bytes saved by each table, raw and `gzip -9`
(of stdin); a negative saving is growth. Sizes do not depend on load.

| Fixture | Advanced | Repeated strings (uses) | All | Paying | Greedy |
| --- | ---: | ---: | ---: | ---: | ---: |
| console-log-sum | 151 / 140 | 0 (0) | 0 / 0 | 0 / 0 | 0 / 0 |
| println-hi | 4,771 / 2,021 | 7 (29) | 34 / -30 | 35 / -27 | 0 / 0 |
| vector-printed | 9,703 / 3,740 | 11 (40) | 8 / -52 | 20 / -14 | 0 / 0 |
| map-printed | 13,514 / 4,825 | 16 (58) | 59 / -82 | 79 / -31 | 0 / 0 |
| seq-pipeline | 24,450 / 7,610 | 20 (73) | 239 / -62 | 250 / -54 | 0 / 0 |
| add | 19 / 39 | 0 (0) | 0 / 0 | 0 / 0 | 0 / 0 |
| rest-fn | 9,465 / 3,177 | 10 (26) | 22 / -38 | 33 / -31 | 0 / 0 |
| keyword-case | 3,428 / 1,324 | 7 (15) | 17 / -25 | 23 / -14 | 0 / 0 |
| record-protocol | 5,750 / 2,423 | 7 (22) | 75 / -23 | 77 / -20 | 0 / 0 |
| pure-defs | 5,640 / 2,372 | 7 (22) | 75 / -22 | 77 / -19 | 0 / 0 |
| keyword-lookups | 29,113 / 9,056 | 28 (105) | 370 / -89 | 381 / -77 | 78 / 1 |
| constant-literals | 20,612 / 6,561 | 19 (64) | 190 / -51 | 201 / -40 | 0 / 0 |
| constant-lookups | 3,808 / 1,453 | 6 (15) | 40 / -18 | 40 / -15 | 0 / 0 |
| three-records | 5,757 / 2,422 | 7 (22) | 75 / -22 | 77 / -20 | 0 / 0 |
| constant-keys | 39,897 / 12,202 | 29 (148) | 686 / -101 | 691 / -96 | 0 / 0 |
| event-maps | 29,341 / 9,123 | 27 (102) | 361 / -84 | 366 / -76 | 0 / 0 |
| vector-only | 4,224 / 1,739 | 3 (6) | 10 / -4 | 10 / -4 | 0 / 0 |
| keyword-only | 3,355 / 1,297 | 5 (11) | 23 / -20 | 23 / -17 | 0 / 0 |
| map-only | 9,146 / 3,194 | 11 (32) | 73 / -52 | 86 / -30 | 0 / 0 |
| atom-str | 2,284 / 904 | 4 (8) | 7 / -15 | 10 / -12 | 4 / 1 |
| set-only | 5,085 / 1,949 | 7 (18) | 49 / -19 | 50 / -9 | 0 / 0 |
| mapv-count | 28,725 / 8,914 | 27 (100) | 325 / -90 | 333 / -79 | 8 / 2 |
| raw-js | 36 / 54 | 0 (0) | 0 / 0 | 0 / 0 | 0 / 0 |
| multi-arity | 318 / 212 | 0 (0) | 0 / 0 | 0 / 0 | 0 / 0 |
| closed-params | 35,923 / 11,755 | 52 (198) | 765 / -166 | 800 / -118 | 0 / 0 |
| debug-logging | 31 / 51 | 0 (0) | 0 / 0 | 0 / 0 | 0 / 0 |
| local-arities | 490 / 278 | 0 (0) | 0 / 0 | 0 / 0 | 0 / 0 |
| total | 295,036 / 98,835 | 310 (1,114) | 3,503 / -1,065 | 3,662 / -803 | 90 / 4 |
| `(read-string "{:a 1}")` | 102,314 / 32,233 | 142 (724) | 2,346 / -563 | 2,514 / -370 | 512 / 18 |

Over the fixtures, the full table saves 1.2% raw and adds 1.1% gzipped;
the paying strings save 1.2% raw and add 0.8% gzipped. Guided by gzip, a
table keeps three strings in three fixtures for 4 gzipped bytes in all
(0.004%), and four strings of the reader program for 18 bytes (0.06%).
Even one-letter names for the 52 most used strings, which would collide
with the program's own bindings (a size-only bound), leave the fixtures
511 bytes (0.5%) larger gzipped.

The repeated strings are short: the kind tags the runtime compares
(`"hash-map"` 31 times in the reader program, `"array-map"` 29, `"list"`
22), `typeof` results (`"string"`, `"number"`), separators (`" "`, `"/"`,
`":"`), protocol keys (`"IEquiv/-equiv"`, `"ISeq/-first"`), the arity
prefix `"cljs$core$IFn$_invoke$arity$"` and a few error messages
(`"Expected keyword"`, `"Unsupported operation"`). Deflate already codes
each repeat as a back-reference, usually inside a longer repeated run
such as `==="array-map"` or `,"array-map")`; a name breaks that run, and
the declaration adds the literal once more. The one long string, the
protocol-name list `"IFn ICounted ... IDrop"` (524 bytes in
`closed-params` and the reader program), occurs once, so a table cannot
shrink it. light-cljs-237 is to replace the protocol-key strings of
dispatch with integers or symbols, which only lowers what a table could
save.

## Program types beside builtin values — October 14, 2026

light-cljs-246 keeps a program type and a builtin vector or keyword from
keeping runtime together ("Phase 2 builtin types, program types" in the
[design](../docs/advanced-optimization.md)). At `--optimize advanced`:

- the host-facing methods and `cnt` fields of vectors, maps and sets take
  the generic operations only where `extend-type` or `specify!` can give
  the runtime's values protocol implementations, not wherever a program
  type exists;
- `protocolHooks` is installed hook by hook, and a program whose protocol
  implementations all belong to its own types finds them on its values
  (`ownMethod`, `builtin` without the tables of `extend-type`);
- structural equality of maps and lookups no longer wait for `makeType`,
  since records answer both through their own methods, and the sequential
  comparison waits for a program type implementing `ISequential`.

**Sizes**, raw and `gzip -9` bytes (of stdin) of `--compile --optimize
LEVEL`, on the 24-CPU Mac (sizes do not depend on load; load average 2.6).
"Before" is master `5e83c97`. `prepare` equals `none` for every fixture but
`pure-defs`, which is 78 bytes smaller there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 395,290 / 94,192 | 397,984 / 95,040 | 151 / 140 | 151 / 140 | 398,000 / 398,000 / 200 |
| println-hi | 395,177 / 94,120 | 397,871 / 94,965 | 4,771 / 2,021 | 4,771 / 2,021 | 398,000 / 398,000 / 4,800 |
| vector-printed | 395,202 / 94,129 | 397,896 / 94,976 | 9,703 / 3,740 | 9,700 / 3,743 | 398,000 / 398,000 / 9,700 |
| map-printed | 395,205 / 94,123 | 397,899 / 94,969 | 13,514 / 4,825 | 13,510 / 4,823 | 398,000 / 398,000 / 14,000 |
| seq-pipeline | 395,521 / 94,242 | 398,215 / 95,089 | 24,450 / 7,610 | 24,427 / 7,603 | 399,000 / 399,000 / 25,000 |
| add | 395,130 / 94,110 | 397,824 / 94,957 | 19 / 39 | 19 / 39 | 398,000 / 398,000 / 100 |
| rest-fn | 395,474 / 94,249 | 398,168 / 95,095 | 9,465 / 3,177 | 9,448 / 3,167 | 399,000 / 399,000 / 9,500 |
| keyword-case | 395,762 / 94,272 | 398,456 / 95,119 | 3,428 / 1,324 | 3,428 / 1,324 | 399,000 / 399,000 / 3,500 |
| record-protocol | 396,083 / 94,442 | 398,777 / 95,290 | 5,750 / 2,423 | 5,750 / 2,423 | 399,000 / 399,000 / 5,800 |
| pure-defs | 396,719 / 94,555 | 399,413 / 95,406 | 5,640 / 2,372 | 5,640 / 2,372 | 400,000 / 400,000 / 5,700 |
| keyword-lookups | 396,254 / 94,379 | 398,948 / 95,228 | 29,113 / 9,056 | 29,110 / 9,053 | 399,000 / 399,000 / 30,000 |
| constant-literals | 396,031 / 94,327 | 398,725 / 95,175 | 20,612 / 6,561 | 20,589 / 6,548 | 399,000 / 399,000 / 21,000 |
| constant-lookups | 396,449 / 94,470 | 399,143 / 95,317 | 3,808 / 1,453 | 3,808 / 1,453 | 400,000 / 400,000 / 3,900 |
| three-records | 398,220 / 94,742 | 400,914 / 95,595 | 5,757 / 2,422 | 5,757 / 2,422 | 401,000 / 401,000 / 5,800 |
| constant-keys | 396,247 / 94,317 | 398,941 / 95,166 | 39,897 / 12,202 | 39,895 / 12,210 | 399,000 / 399,000 / 40,000 |
| event-maps | 395,779 / 94,286 | 398,473 / 95,134 | 29,341 / 9,123 | 29,338 / 9,115 | 399,000 / 399,000 / 30,000 |
| vector-only | 395,329 / 94,213 | 398,023 / 95,061 | 4,224 / 1,739 | 4,221 / 1,739 | 399,000 / 399,000 / 4,300 |
| keyword-only | 395,297 / 94,195 | 397,991 / 95,042 | 3,355 / 1,297 | 3,355 / 1,297 | 398,000 / 398,000 / 3,400 |
| map-only | 395,335 / 94,204 | 398,029 / 95,052 | 9,146 / 3,194 | 9,143 / 3,190 | 399,000 / 399,000 / 9,200 |
| atom-str | 395,345 / 94,202 | 398,039 / 95,050 | 2,284 / 904 | 2,284 / 904 | 399,000 / 399,000 / 2,300 |
| set-only | 395,366 / 94,224 | 398,060 / 95,071 | 5,085 / 1,949 | 5,078 / 1,955 | 399,000 / 399,000 / 5,100 |
| mapv-count | 395,357 / 94,213 | 398,051 / 95,060 | 28,725 / 8,914 | 28,722 / 8,917 | 399,000 / 399,000 / 29,000 |
| raw-js | 396,129 / 94,372 | 398,823 / 95,222 | 36 / 54 | 36 / 54 | 399,000 / 399,000 / 100 |
| multi-arity | 397,958 / 94,638 | 400,652 / 95,493 | 318 / 212 | 318 / 212 | 401,000 / 401,000 / 400 |
| closed-params | 397,693 / 94,782 | 400,387 / 95,637 | 35,923 / 11,755 | 12,996 / 4,960 | 401,000 / 401,000 / 13,000 |
| debug-logging | 396,449 / 94,557 | 399,143 / 95,406 | 31 / 51 | 31 / 51 | 400,000 / 400,000 / 100 |
| local-arities | 398,381 / 94,738 | 401,075 / 95,593 | 490 / 278 | 490 / 278 | 402,000 / 402,000 / 500 |

`closed-params` drops by 64% (light-cljs-243 asked for 25%). No fixture
grows; those with vectors, maps or sets lose up to 23 bytes, of which four
gain 3 to 8 gzipped bytes.

Other programs at `advanced`, raw and gzip bytes:

| Program | Before | After |
| --- | ---: | ---: |
| `(defrecord Pt [x y]) (loop [i 0 acc 0] (if (< i 10) (recur (inc i) (+ acc (:x (->Pt i 0)) (nth [1 2 3] (rem i 3)))) acc))` | 35,426 / 11,507 | 10,242 / 4,030 |
| the same with `(deftype Pt [x y])` and `(.-x ...)` | 33,285 / 10,888 | 9,745 / 3,847 |
| `(defrecord Pt [x y]) (js/console.log (:x (->Pt 1 0)) (.toString :a))` | 35,274 / 11,387 | 9,895 / 3,751 |
| `(defrecord Pt [x y]) (loop ...)` with `(:x (->Pt i 0))` alone | 5,748 / 2,433 | 5,748 / 2,433 |
| `(loop ...)` with `(nth [1 2 3] (rem i 3))` alone | 4,296 / 1,789 | 4,293 / 1,791 |
| `(deftype T [] ICounted (-count [_] 7)) (count (T.))` | 33,027 / 10,728 | 27,180 / 8,726 |

The record and the vector together now keep 10,242 bytes against 5,748
and 4,296 alone (target 12,000). `count` of a program type still keeps
the protocol objects and the native tests through `implementsProtocol`.

The shared runtime grows by 2,694 bytes (0.7%), 848 gzipped: `ownMethod`,
`externalMethod`, the split hook installs and their comments. The `none`
and `prepare` ceilings rise by 2,000 or 3,000 for that growth (each to its
new size rounded up to the next 1,000); the `advanced` ceilings of
`closed-params` (36,000 to 13,000) and `vector-printed` (9,800 to 9,700)
drop.

## Named record slots — October 14, 2026

light-cljs-250 installs a record's core protocol method where the program
names its slot (`cljs$core$ILookup$_lookup$arity$3`) as a string, property
or method, or in `js*` text, besides where an operation dispatching it is
kept ("Named record slots" in the [design](../docs/advanced-optimization.md)).

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize advanced`, on
the 24-CPU Mac (sizes do not depend on load; load average 3.0). "Before" is
master `3673641`. No size-ratchet fixture changes size at any level, and the
runtime is unchanged. With `(defrecord R [x]) (def r (->R 1))` first:

| Program | Before | After |
| --- | ---: | ---: |
| `(.call (aget r "cljs$core$ILookup$_lookup$arity$3") r r "missing" "nf")` | 5,695 / 2,395 (throws) | 9,796 / 3,698 (`"nf"`) |
| `(array (.equiv [1] [1]) (.call (aget r "cljs$core$ILookup$_lookup$arity$3") r "missing" "nf"))` | 10,099 / 3,935 (throws) | 12,358 / 4,598 |
| `(.-x r)`, naming no slot | 5,638 / 2,368 | 5,638 / 2,368 |

## Callback parameter types — October 14, 2026

light-cljs-243 types the parameters of functions passed to core operations
whose rows in `src/core_operations.tsv` give a callback shape (`map`,
`filter`, `reduce`, `swap!` and 28 more), from the elements of their
collections and their other arguments ("Phase 2 callback parameter extension
points" in the [design](../docs/advanced-optimization.md)). The new ratchet
fixture `callbacks` passes record-reading functions to `filter`, `reduce`
and `map`, and a keyword-comparing one to `swap!`.

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL`, on the
24-CPU Mac (sizes do not depend on load; load average 2.9). "Before" is
master `3673641`.

| Fixture | None before → after | Advanced before → after |
| --- | --- | --- |
| every existing fixture (28) | identical bytes | identical bytes |
| closed-params | 400,387 / 95,659 → same | 12,996 / 4,986 → same |
| callbacks (new) | 399,835 / 95,520 → same | 42,345 / 13,298 → 42,261 / 13,283 |

The new fixture's ceilings are 400,000 / 400,000 / 43,000. In `callbacks`
the record reads inside `heavy?`, `add-cost` and the `map` callback become
field reads, so the `:qty`, `:price` and `:label` keywords are no longer
built (84 bytes).

`closed-params` does not change with this pellet. Its functions have been
closed since light-cljs-229, so their parameters were already typed. The
pellet's target was 25% below the 37,763 bytes it measured when filed; the
fixture is 12,996 bytes (66% smaller) because light-cljs-246 stopped a
program type and a vector or keyword from keeping runtime together.

Typed lowerings make the code faster but not always smaller: a string
`count` written as `(.length s)` is longer than a renamed runtime call, and
the direct vector `nth` path is longer than `rt$nth`. A program whose
callbacks only count strings went from 32,709 to 32,717 bytes on master
`0d00e45`, because `count` stays in the runtime for its other calls. A
lowering shrinks output only when it removes a runtime root. For that
reason no specialized clones were added for call sites that dominate an
`any` join: a clone copies a body.

## Protocol method parameter types — October 14, 2026

light-cljs-253 types the parameters of the methods that implement a program
protocol (`deftype`, `defrecord`, `reify`, `specify!`, `extend-type` of a
program type) from the protocol method's calls, when the program makes every
call of the method and the call's receiver may run the method ("Phase 2
protocol method parameter extension points" in the
[design](../docs/advanced-optimization.md)). The new ratchet fixture
`protocol-params` prices records through two types implementing one protocol.

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize LEVEL`, on the
24-CPU Mac (sizes do not depend on load; load average 8.5). "Before" is master
`e5ee884`; the size ratchet table is identical with and without this change
except for the new fixture.

| Fixture | None before → after | Advanced before → after |
| --- | --- | --- |
| every existing fixture (29) | identical bytes | identical bytes |
| protocol-params (new) | 401,140 / 95,777 → same | 47,413 / 14,863 → 47,267 / 14,820 |

The new fixture's ceilings are 402,000 / 402,000 / 48,000. Its methods'
`item` parameters are records of one type, so `(:qty item)`, `(:price item)`
and `(:label item)` become field reads and those keywords are no longer
built (146 bytes), and `tag` tests its boolean `flag` without `truth()`.

## Atom states — October 14, 2026

light-cljs-252 types the state that `swap!` and `swap-vals!` pass their
function from what the atom holds, when the program makes every use of the
atom itself: its initial value and every value `reset!`, `reset-vals!`,
`compare-and-set!` and the `swap!` functions store ("Phase 2 atom state
extension points" in the [design](../docs/advanced-optimization.md)).

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize advanced`, on the
24-CPU Mac (sizes do not depend on load; load average 5.2). "Before" is this
change with atom tracking switched off, on master `f76712a`. The size ratchet
table is identical before and after at every level: the `swap!` callback of
the `callbacks` fixture now has a `number` state, but it only adds to it, and
`+` is an operator for every operand already.

| Program | Advanced before → after |
| --- | --- |
| every ratchet fixture | identical bytes |
| `toggle!` and `text` below | 21,147 / 7,078 → 21,107 / 7,062 |

```clojure
(def flag (atom nil))
(defn toggle! [] (swap! flag (fn [f] (if f false true))))
(toggle!)
(let [text (atom "")]
  (swap! text (fn [s k] (str s (count s) k)) "a")
  (pr-str [@flag @text]))
```

The `toggle!` callback tests its `nil`/`boolean` state without `truth()`, and
the `text` callback counts its string state with `.length`, which drops
`rt$count` from the program's roots.

## Protocol tests by name — October 14, 2026

light-cljs-251 has the generic operations test core protocols by name
("Protocol tests by name" in the
[design](../docs/advanced-optimization.md)): where no `extend-type` or
`specify!` exists, a value satisfies the protocols it declares
(`ownProtocol`) and otherwise what the runtime's values satisfy natively,
so a program type keeps neither `satisfies` nor `implementsProtocol`, nor
the protocol objects and native tests they read.

**Sizes**, raw and `gzip -9` bytes (of stdin) of `--compile --optimize
LEVEL`, on the 24-CPU Mac (sizes do not depend on load; load average 7.8).
"Before" is master `e5ee884`. `prepare` equals `none` for every fixture but
`pure-defs`, which is 78 bytes smaller there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 397,984 / 95,040 | 398,805 / 95,292 | 151 / 140 | 151 / 140 | 399,000 / 399,000 / 200 |
| println-hi | 397,871 / 94,965 | 398,692 / 95,219 | 4,771 / 2,021 | 4,771 / 2,021 | 399,000 / 399,000 / 4,800 |
| vector-printed | 397,896 / 94,976 | 398,717 / 95,231 | 9,700 / 3,743 | 9,659 / 3,719 | 399,000 / 399,000 / 9,700 |
| map-printed | 397,899 / 94,969 | 398,720 / 95,223 | 13,510 / 4,823 | 13,453 / 4,807 | 399,000 / 399,000 / 14,000 |
| seq-pipeline | 398,215 / 95,089 | 399,036 / 95,342 | 24,427 / 7,603 | 24,360 / 7,573 | 400,000 / 400,000 / 25,000 |
| add | 397,824 / 94,957 | 398,645 / 95,210 | 19 / 39 | 19 / 39 | 399,000 / 399,000 / 100 |
| rest-fn | 398,168 / 95,095 | 398,989 / 95,348 | 9,448 / 3,167 | 9,410 / 3,147 | 400,000 / 400,000 / 9,500 |
| keyword-case | 398,456 / 95,119 | 399,277 / 95,374 | 3,428 / 1,324 | 3,428 / 1,324 | 400,000 / 400,000 / 3,500 |
| record-protocol | 398,777 / 95,290 | 399,598 / 95,543 | 5,750 / 2,423 | 5,750 / 2,423 | 400,000 / 400,000 / 5,800 |
| pure-defs | 399,413 / 95,406 | 400,234 / 95,656 | 5,640 / 2,372 | 5,640 / 2,372 | 401,000 / 401,000 / 5,700 |
| keyword-lookups | 398,948 / 95,228 | 399,769 / 95,481 | 29,110 / 9,053 | 29,032 / 9,025 | 400,000 / 400,000 / 30,000 |
| constant-literals | 398,725 / 95,175 | 399,546 / 95,428 | 20,589 / 6,548 | 20,533 / 6,513 | 400,000 / 400,000 / 21,000 |
| constant-lookups | 399,143 / 95,317 | 399,964 / 95,570 | 3,808 / 1,453 | 3,808 / 1,453 | 400,000 / 400,000 / 3,900 |
| three-records | 400,914 / 95,595 | 401,735 / 95,847 | 5,757 / 2,422 | 5,757 / 2,422 | 402,000 / 402,000 / 5,800 |
| constant-keys | 398,941 / 95,166 | 399,762 / 95,420 | 39,895 / 12,210 | 39,788 / 12,188 | 400,000 / 400,000 / 40,000 |
| event-maps | 398,473 / 95,134 | 399,294 / 95,389 | 29,338 / 9,115 | 29,266 / 9,086 | 400,000 / 400,000 / 30,000 |
| vector-only | 398,023 / 95,061 | 398,844 / 95,315 | 4,221 / 1,739 | 4,221 / 1,739 | 399,000 / 399,000 / 4,300 |
| keyword-only | 397,991 / 95,042 | 398,812 / 95,296 | 3,355 / 1,297 | 3,355 / 1,297 | 399,000 / 399,000 / 3,400 |
| map-only | 398,029 / 95,052 | 398,850 / 95,305 | 9,143 / 3,190 | 9,116 / 3,185 | 399,000 / 399,000 / 9,200 |
| atom-str | 398,039 / 95,050 | 398,860 / 95,303 | 2,284 / 904 | 2,284 / 904 | 399,000 / 399,000 / 2,300 |
| set-only | 398,060 / 95,071 | 398,881 / 95,325 | 5,078 / 1,955 | 5,062 / 1,946 | 399,000 / 399,000 / 5,100 |
| mapv-count | 398,051 / 95,060 | 398,872 / 95,314 | 28,722 / 8,917 | 28,671 / 8,886 | 399,000 / 399,000 / 29,000 |
| raw-js | 398,823 / 95,222 | 399,644 / 95,474 | 36 / 54 | 36 / 54 | 400,000 / 400,000 / 100 |
| multi-arity | 400,652 / 95,493 | 401,473 / 95,743 | 318 / 212 | 318 / 212 | 402,000 / 402,000 / 400 |
| closed-params | 400,387 / 95,637 | 401,208 / 95,887 | 12,996 / 4,960 | 12,996 / 4,960 | 402,000 / 402,000 / 13,000 |
| debug-logging | 399,143 / 95,406 | 399,964 / 95,658 | 31 / 51 | 31 / 51 | 400,000 / 400,000 / 100 |
| local-arities | 401,075 / 95,593 | 401,896 / 95,845 | 490 / 278 | 490 / 278 | 402,000 / 402,000 / 500 |
| callbacks | 399,835 / 95,498 | 400,656 / 95,749 | 42,261 / 13,257 | 37,186 / 11,741 | 401,000 / 401,000 / 38,000 |

No fixture grows at `advanced`. `callbacks`, whose records meet `filter`,
`reduce` and `map`, loses 5,075 bytes (12%) and its ceiling drops from
43,000 to 38,000; eleven others lose 16 to 107 bytes, since the generic
operations no longer read the protocol objects. The `none` and `prepare`
ceilings of 20 fixtures rise by 1,000 for the shared runtime's growth of
821 bytes (0.2%; 252 gzipped): `ownProtocol`, the two hook installs with
their `when:` lists, and the protocol names at the call sites, less 8
bytes at each of the 22 reads of the renamed `protocolHooks.implements`.

Other programs at `advanced`, raw and gzip bytes:

| Program | Before | After |
| --- | ---: | ---: |
| `(deftype T [] ICounted (-count [_] 7)) (count (T.))` | 27,180 / 8,726 | 21,678 / 7,157 |
| `(deftype T [] ICounted (-count [_] 7)) (counted? (T.))` | 12,375 / 4,304 | 6,181 / 2,563 |
| `(deftype T [] ISeqable (-seq [_] (list 1 2))) (seq? (T.))` | 28,136 / 8,984 | 22,634 / 7,413 |
| `(defrecord R [x]) (map? (->R 1))` | 12,576 / 4,381 | 5,984 / 2,509 |
| `(count (.split "a,b" ","))` | 8,985 / 2,973 | 8,948 / 2,952 |
| `(deftype T [] ICounted (-count [_] 7)) (.-x (T.))` | 5,437 / 2,296 | 5,437 / 2,296 |

`count` of a program type stays above the 12,000 bytes the pellet asked
for: `count` alone keeps 8,948 bytes and the type 5,437, and the rest is
the record effects targeting `makeType` and the core dispatchers, which
this change does not touch.

## Count walks and record hooks — October 14, 2026

The second part of light-cljs-251 (with the two splits light-cljs-254
filed) has `count` walk a seq only where the program builds a value that
needs it ("Count walks only what needs walking" in the
[design](../docs/advanced-optimization.md)): through `kindHooks.countSeq`,
which the builders of cells, lazy seqs and host iterators' seqs,
`iteration`, `Eduction`, extensions and a program type implementing
`ISeqable` install. The map hooks a record can reach wait for
`recordMethods` instead of `makeType`, and each core dispatcher and its
`protocolHooks` install is a unit of its own.

**Sizes**, raw and `gzip -9` bytes (of stdin) of `--compile --optimize
LEVEL`, on the 24-CPU Mac (sizes do not depend on load; load average 9.4).
"Before" is the first part of light-cljs-251 (its two earlier commits) on
master `468e71e`. `prepare` equals `none` for every fixture but `pure-defs`,
which is 78 bytes smaller there.

| Fixture | None before | None after | Advanced before | Advanced after | Ceilings after |
| --- | ---: | ---: | ---: | ---: | --- |
| console-log-sum | 398,805 / 95,292 | 399,203 / 95,432 | 151 / 140 | 151 / 140 | 400,000 / 400,000 / 200 |
| println-hi | 398,692 / 95,219 | 399,090 / 95,359 | 4,771 / 2,021 | 4,771 / 2,021 | 400,000 / 400,000 / 4,800 |
| vector-printed | 398,717 / 95,231 | 399,115 / 95,369 | 9,659 / 3,719 | 9,659 / 3,719 | 400,000 / 400,000 / 9,700 |
| map-printed | 398,720 / 95,223 | 399,118 / 95,362 | 13,453 / 4,807 | 13,453 / 4,807 | 400,000 / 400,000 / 14,000 |
| seq-pipeline | 399,036 / 95,342 | 399,434 / 95,481 | 24,360 / 7,573 | 24,348 / 7,568 | 400,000 / 400,000 / 25,000 |
| add | 398,645 / 95,210 | 399,043 / 95,350 | 19 / 39 | 19 / 39 | 400,000 / 400,000 / 100 |
| rest-fn | 398,989 / 95,348 | 399,387 / 95,487 | 9,410 / 3,147 | 9,400 / 3,145 | 400,000 / 400,000 / 9,400 |
| keyword-case | 399,277 / 95,374 | 399,675 / 95,512 | 3,428 / 1,324 | 3,428 / 1,324 | 400,000 / 400,000 / 3,500 |
| record-protocol | 399,598 / 95,543 | 399,996 / 95,682 | 5,750 / 2,423 | 5,750 / 2,423 | 400,000 / 400,000 / 5,800 |
| pure-defs | 400,234 / 95,656 | 400,632 / 95,796 | 5,640 / 2,372 | 5,640 / 2,372 | 401,000 / 401,000 / 5,700 |
| keyword-lookups | 399,769 / 95,481 | 400,167 / 95,620 | 29,032 / 9,025 | 29,020 / 9,030 | 401,000 / 401,000 / 30,000 |
| constant-literals | 399,546 / 95,428 | 399,944 / 95,567 | 20,533 / 6,513 | 20,521 / 6,519 | 400,000 / 400,000 / 21,000 |
| constant-lookups | 399,964 / 95,570 | 400,362 / 95,709 | 3,808 / 1,453 | 3,808 / 1,453 | 401,000 / 401,000 / 3,900 |
| three-records | 401,735 / 95,847 | 402,133 / 95,985 | 5,757 / 2,422 | 5,757 / 2,422 | 403,000 / 403,000 / 5,800 |
| constant-keys | 399,762 / 95,420 | 400,160 / 95,558 | 39,788 / 12,188 | 39,776 / 12,175 | 401,000 / 401,000 / 40,000 |
| event-maps | 399,294 / 95,389 | 399,692 / 95,526 | 29,266 / 9,086 | 29,254 / 9,078 | 400,000 / 400,000 / 30,000 |
| vector-only | 398,844 / 95,315 | 399,242 / 95,453 | 4,221 / 1,739 | 4,221 / 1,739 | 400,000 / 400,000 / 4,300 |
| keyword-only | 398,812 / 95,296 | 399,210 / 95,434 | 3,355 / 1,297 | 3,355 / 1,297 | 400,000 / 400,000 / 3,400 |
| map-only | 398,850 / 95,305 | 399,248 / 95,443 | 9,116 / 3,185 | 9,116 / 3,185 | 400,000 / 400,000 / 9,200 |
| atom-str | 398,860 / 95,303 | 399,258 / 95,442 | 2,284 / 904 | 2,284 / 904 | 400,000 / 400,000 / 2,300 |
| set-only | 398,881 / 95,325 | 399,279 / 95,463 | 5,062 / 1,946 | 5,062 / 1,946 | 400,000 / 400,000 / 5,100 |
| mapv-count | 398,872 / 95,314 | 399,270 / 95,452 | 28,671 / 8,886 | 28,659 / 8,889 | 400,000 / 400,000 / 29,000 |
| raw-js | 399,644 / 95,474 | 400,042 / 95,611 | 36 / 54 | 36 / 54 | 401,000 / 401,000 / 100 |
| multi-arity | 401,473 / 95,743 | 401,871 / 95,882 | 318 / 212 | 318 / 212 | 402,000 / 402,000 / 400 |
| closed-params | 401,208 / 95,887 | 401,606 / 96,026 | 12,996 / 4,960 | 12,996 / 4,960 | 402,000 / 402,000 / 13,000 |
| debug-logging | 399,964 / 95,658 | 400,362 / 95,798 | 31 / 51 | 31 / 51 | 401,000 / 401,000 / 100 |
| local-arities | 401,896 / 95,845 | 402,294 / 95,983 | 490 / 278 | 490 / 278 | 403,000 / 403,000 / 500 |
| callbacks | 400,656 / 95,749 | 401,054 / 95,888 | 37,186 / 11,741 | 36,161 / 11,495 | 402,000 / 402,000 / 37,000 |
| protocol-params | 401,961 / 96,020 | 402,359 / 96,158 | 42,315 / 13,332 | 41,291 / 13,092 | 403,000 / 403,000 / 42,000 |

No fixture grows at `advanced`: `callbacks` loses 1,025 bytes and its
ceiling drops from 38,000 to 37,000, `protocol-params` 1,024 and its
ceiling, which the first part did not lower, from 48,000 to 42,000, and
six fixtures that keep the seq walk lose 10 to 12 bytes (`rest-fn`'s
ceiling drops from 9,500 to 9,400), since `count` no longer tests lazy
seqs and cells itself; three of those gain 3 to 6 gzipped bytes. The
`none` and `prepare` ceilings of 20 fixtures rise by 1,000 for the shared runtime's growth of 398 bytes (0.1%; 140
gzipped): the two hook installs, the `const` of each split dispatcher, and
their comments.

Other programs at `advanced`, raw and gzip bytes:

| Program | Master `e5ee884` | Before | After |
| --- | ---: | ---: | ---: |
| `(deftype T [] ICounted (-count [_] 7)) (count (T.))` | 27,180 / 8,726 | 21,678 / 7,157 | 7,662 / 3,043 |
| `(count (.split "a,b" ","))` | 8,985 / 2,973 | 8,948 / 2,952 | 1,356 / 593 |
| `(defrecord R [x]) (count (->R 1))` | | 23,575 / 7,665 | 7,947 / 3,156 |
| `(deftype T [x]) (first (seq [(.-x (T. 1))]))` | | 21,637 / 7,150 | 19,288 / 6,482 |
| `(deftype T [] ISeqable (-seq [_] (list 1 2))) (seq? (T.))` | 28,136 / 8,984 | 22,634 / 7,413 | 17,647 / 5,972 |
| `(deftype S [] ISeqable (-seq [_] (list 1 2 3))) (count (S.))` | | 22,588 / 7,397 | 17,601 / 5,957 |
| `(count (lazy-seq [1 2]))` | | 12,389 / 4,023 | 12,378 / 4,032 |
| `(deftype T [] ICounted (-count [_] 7)) (counted? (T.))` | 12,375 / 4,304 | 6,181 / 2,563 | 6,181 / 2,563 |
| `(defrecord R [x]) (map? (->R 1))` | 12,576 / 4,381 | 5,984 / 2,509 | 5,984 / 2,509 |

`count` of a program type now keeps 7,662 bytes, under the 12,000 the
pellet asked for: the type's 5,437 and `count` with the protocol hooks it
reads. Without the seq walk it reaches neither the record hooks nor the
core dispatchers; those two changes save where a program keeps the walk
or a seq, as the `deftype` with a `seq` and `count` of a type implementing
`ISeqable` above.

## Atom results — October 14, 2026

light-cljs-256 types what `deref`, `reset!`, `swap!`, `reset-vals!` and
`swap-vals!` return on an atom the program makes every use of, from what the
atom holds ("Phase 2 atom state extension points" in the
[design](../docs/advanced-optimization.md)).

**Sizes**, raw and `gzip -9` bytes of `--compile --optimize advanced`, on the
24-CPU Mac (sizes do not depend on load; load average 4.3). "Before" is
master `421b72d`. The size ratchet table is identical before and after at
every level: the `callbacks` fixture's `@counter` is now a number, but it is
an operand of `+`, which is an operator for every operand already.

| Program | Advanced before → after |
| --- | --- |
| every ratchet fixture | identical bytes |
| program below | 24,158 / 7,786 → 24,118 / 7,766 |

```clojure
(def counter (atom 0))
(defn tick! [] (swap! counter inc))
(tick!)
(let [flag (atom false)
      text (atom "")]
  (swap! flag not)
  (swap! text str "ab")
  (pr-str [(if @flag :on :off) (count @text) (pos? @counter)]))
```

`(if @flag ...)` loses `truth()` and `(count @text)` becomes `.length`, which
drops `rt$count` from the program's roots; `pos?` was already `> 0` for
every operand.

## Reassigned call properties — October 14, 2026

light-cljs-257 states in the Advanced-mode contract of the
[design](../docs/advanced-optimization.md) that reassigning a function's
`call` or `apply` property is unsupported, instead of detecting such writes
and turning off direct calls, inlining and the call summaries of inference
for the functions they may reach. No ratchet fixture writes a `call`
property, so detection would change none of them; what it would cost is
what those passes save in a program that does. As a lower bound, turning off
inlining alone (`ApplicationOptions::no_inlining` in the size ratchet's
`emit`, `cargo nextest run --test compiler size_ratchet:: --no-capture`, on
the 24-CPU Mac at master `7514c6e`; sizes do not depend on load) changes 7
of the 29 fixtures at `Advanced`, by 29,246 bytes in all:

| Fixture | Advanced | without inlining |
| --- | ---: | ---: |
| constant-literals | 20,521 | 31,845 |
| event-maps | 29,254 | 29,302 |
| raw-js | 36 | 250 |
| multi-arity | 318 | 756 |
| closed-params | 12,996 | 13,119 |
| debug-logging | 31 | 17,101 |
| callbacks | 36,161 | 36,190 |

Upstream ClojureScript at `a5e94c9`, through `cljs.build.api` with
`:target :nodejs` in Node 24, prints `[:off :off :off]` for the pellet's
program at `:none`, `:simple` and `:advanced` alike (its analyzer types
`(f 1)` from `f`'s bodies and tests it without `truth_`), and for calls whose
results it does not type, `[:replaced :replaced :replaced]` at `:none` and
`[:body :body :body]` at `:advanced`, where `:static-fns` calls known
functions directly. Its `swap!` and `map` call the replacement at every
level.

## Persistent vector trie — October 14, 2026

light-cljs-234 replaces the vector representation, a frozen array copied on
every update, with upstream's PersistentVector: a 32-way trie of frozen
32-item leaves and a tail of up to 32 items (docs/compiler.md, "Collections").
`conj` copies only the tail (and every 32nd item moves a frozen copy of it into
the trie), `nth`, `assoc` and `pop` follow one path, and a transient vector owns
copies of the tail and of the nodes it changes. Further changes, each needed for
a workload below to come near upstream:

- A vector's brand and store became a private class field (`vectors` keeps a
  WeakMap's `has`, `get` and `set`). Adding a WeakMap entry for each new vector
  alone cost about 4.4 ms per 20,000 vectors in Node, nine times upstream's whole
  `vector-conj`; a private field cannot be forged or copied by host code either.
- `reduce` of an integer range with an initial value walks the numbers, as
  upstream's IntegerRange does, instead of a seq: the range walk, not the
  vector, was most of `vector-conj` and all of `vector-into` once `conj` was
  cheap.
- `subvec` is a view of its vector's store (`VectorView`), O(1) as upstream's
  Subvec, and `reduce` of a vector, a view or a ChunkedSeq over one walks the
  trie's leaves instead of a copy of its items.
- `into`, `mapv` and `filterv` `conj!` onto their own transient past the
  protocol dispatch where no protocol implementation exists, and `mapv` of one
  collection and `filterv` are reductions into a transient, as upstream.
- `nth` tests for a vector before the other kinds, and the runtime's own
  ChunkedSeq steps store no null metadata or hash entries.

Command (the collection workloads of "Collection throughput against upstream"
that build or read vectors), Node v24.18.0, the same Mac, after rebasing onto
master `c2f1711`; another worker was active, and the one-minute load average
was 6.7–7.9 over the rounds:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only vector-conj,vector-nth,subvec-reduce,vector-into --output DIR
```

Median ms per call; "before" is the baseline section above (load 2.4–11.2):

| Workload | n | Before | After | Upstream (this run) | After / upstream |
| --- | ---: | ---: | ---: | ---: | ---: |
| vector-conj | 2,000 | 5.63 | 0.171 | 0.088 | 1.9× |
| vector-conj | 20,000 | 717 | 1.122 | 0.475 | 2.4× |
| vector-nth | 2,000 | 0.082 | 0.043 | 0.018 | 2.3× |
| vector-nth | 20,000 | 0.566 | 0.156 | 0.124 | 1.3× |
| subvec-reduce | 2,000 | 0.162 | 0.035 | 0.017 | 2.1× |
| subvec-reduce | 20,000 | 1.54 | 0.175 | 0.079 | 2.2× |
| vector-into | 2,000 | 1.93 | 0.106 | 0.029 | 3.7× |
| vector-into | 20,000 | 19.8 | 0.800 | 0.190 | 4.2× |

`vector-conj` grows 6.6× from 2,000 to 20,000 items (upstream 5.4×), linear
where it was quadratic (127×). `vector-into` is still about 4× upstream: the
transient vector's `conj!` goes through `conjTransient` and the `transients`
WeakMap, where upstream's is a method call.

Outside the benchmark driver, a throwaway script timing 200 calls of each at
`advanced` in Node (load about 10), against the master build of the morning:
`(count (mapv inc v))` over a 20,000-item vector went from 52.8 ms to 0.60 ms,
`(count (filterv odd? v))` from 95.5 ms to 0.52 ms, and `(first (rseq v))` from
10.7 ms to 0.63 ms; with the trie's first slice alone, before the ChunkedSeq
and `rseq` changes, a seq of a long vector cost about 3 µs an item, and those
three took 81, 0.85 and 59 ms.

Sizes (bytes), re-measured after rebasing onto master `c2f1711` ("before" is
that master). Every program that tests for a vector keeps the `vectors` class
(about 180 bytes minified instead of `new WeakMap()`), and one that builds
vectors the trie (vector-only: `vectorTrie`, `vectorLeaf`, `vectorSlice`,
`vectorItems`; mapv-count also the transient trie, `conj` and `reduce`). One
that iterates, prints or compares vectors also keeps the items that read a
caller's array as they are read (`vectorStorage`, about 180 bytes). The user
accepted this growth for light-cljs-234, so the `advanced` ceilings of the
fixtures that grew rise to their new sizes, rounded up. The shared runtime
grows by about 17 KB with its comments (console-log-sum at `none`: 399,203 to
416,592), and every `none` and `prepare` ceiling rises to the new size rounded
up to the next 1,000:

| Fixture | Advanced before | Advanced after |
| --- | ---: | ---: |
| vector-printed | 9,659 | 10,502 |
| map-printed | 13,453 | 14,312 |
| seq-pipeline | 24,348 | 25,131 |
| rest-fn | 9,400 | 9,765 |
| keyword-case | 3,428 | 3,612 |
| keyword-lookups | 29,020 | 30,248 |
| constant-literals | 20,521 | 21,312 |
| constant-lookups | 3,808 | 3,992 |
| constant-keys | 39,776 | 41,330 |
| event-maps | 29,254 | 30,482 |
| vector-only | 4,221 | 5,023 |
| keyword-only | 3,355 | 3,539 |
| map-only | 9,116 | 9,943 |
| atom-str | 2,284 | 2,464 |
| set-only | 5,062 | 5,246 |
| mapv-count | 28,659 | 31,154 |
| closed-params | 12,996 | 13,826 |
| callbacks | 36,161 | 37,387 |
| protocol-params | 41,291 | 42,552 |

## Record construction — October 14, 2026

light-cljs-258 constructs record and `deftype` instances without the
per-instance work `makeType` did: it munged every field name (`mungeMember`,
an `Array.from` and a `join`), defined an accessor per field over a fresh
object, wrote the `typeFields`, `recordTypes`, `mapKinds` and `maps` stores
and read `ifnValues` for the type's prototype, and a record's positional
factory built and spread an array. Now the emitter writes each type's field
assignments once (`this.w = a0; this.h = a1; this.__meta = a2; ...`), a
record's factory passes its fields on (`(a0, a1) => new Rect(a0, a1, null,
null, null)`), and the only per-instance work besides the fields and the two
masks is a private `TypeBrand` field, which the store methods for types and
records read ("User-defined types" in [compiler.md](../docs/compiler.md)).

The pellet's acceptance command, on the 24-CPU Mac (Node v24.18.0), most
runs while another worker built and tested on it:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only record-protocol --output DIR
```

"Before" is master `c2f1711`; the first two "after" runs are this change on
`c2f1711`, the last on `5501372` (persistent vectors, which this workload
does not use) with the review fixes. Milliseconds per call `(f 2000)`,
median / minimum; upstream is the same workload at `:advanced`, timed in the
same rounds, so its column moves with the load:

| Build | One-minute load (median of rounds) | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream |
| --- | ---: | ---: | ---: | ---: |
| before | 16.2 | 10.234 / 9.688 | 0.025 / 0.022 | 409× |
| after | 12.9 | 0.042 / 0.033 | 0.025 / 0.019 | 1.7× |
| after | 35.0 | 0.044 / 0.038 | 0.027 / 0.020 | 1.6× |
| after, rebased | 4.1 | 0.038 / 0.037 | 0.019 / 0.018 | 1.96× |

The other levels of the before and last runs: Node at `none` 10.03 → 0.288
ms and at `prepare` 9.89 → 0.291 ms; QuickJS at `advanced` 16.6 → 1.53 ms
and at `none` 21.2 → 5.36 ms.

The quiet run is within 2× by a narrow margin, and the margin is what V8's
tiers leave. A sample's median mixes them: of its 20 timed calls (after 10
untimed), the first few still run in the mid tier (Maglev), at about 55 µs
a call, and the rest in TurboFan, at about 17 µs, what an upstream-shaped
constructor without the brand takes in the same program. In Maglev the
private field costs about 15 ns per instance (`node --no-turbofan` on the
compiled program: 53 µs a call, 23 µs without it), and TurboFan removes the
cost. Reading `new.target` to let a constructor re-initialize an existing
instance added 11 µs a call in Maglev, so constructors do not
("User-defined types"). Most of the rest is the program's call shape: the
protocol call is an immediately applied arrow around `->Rect.call(null, i,
2)`, and direct calls halve the Maglev time of an upstream-shaped program
(19 → 9.5 µs a call). Of the per-instance work this pellet removed, the
`ifnValues` read was the last to go, at about a fifth of the time left once
the stores and accessors were gone. An instance now calls through its
prototype's `IFn` table, as `ifnMethods` already did for any value without a
table of its own, so an extension that replaces the table reaches existing
instances, as upstream's prototype methods do.

**Sizes**, bytes of `--compile --optimize LEVEL` (`cargo nextest run --test
compiler size_ratchet:: --no-capture`), "before" on master `5501372`. At
`advanced`, programs without records lose up to 34 bytes or keep their
size: `typeFields`, `recordTypes` and `reifyValues` are units of their own,
so a program keeps only the stores it reads. `none` and `prepare` embed the
whole runtime, which grew by 1,952 bytes (the brand, the store effects,
`typeMasks` and their comments), so their ceilings rise by 2,000 bytes
(3,000 for `closed-params`, which was 5 bytes under its ceiling). No
`advanced` ceiling rises; five fall.

| Fixture | Advanced before | Advanced after | Ceiling after (none / prepare / advanced) |
| --- | ---: | ---: | --- |
| record-protocol | 5,750 | 5,475 | 420,000 / 420,000 / 5,500 |
| three-records | 5,757 | 5,468 | 422,000 / 422,000 / 5,500 |
| pure-defs | 5,640 | 5,365 | 421,000 / 420,000 / 5,400 |
| closed-params | 13,826 | 13,775 | 422,000 / 422,000 / 14,000 |
| callbacks | 37,387 | 37,489 | 421,000 / 421,000 / 38,000 |
| protocol-params | 42,552 | 42,683 | 422,000 / 422,000 / 43,000 |
| keyword-case | 3,612 | 3,580 | 420,000 / 420,000 / 3,600 |
| vector-only | 5,023 | 4,991 | 419,000 / 419,000 / 5,000 |

`callbacks` and `protocol-params` grow, by 102 and 131 bytes. They keep
`mapKinds` as well, so all three record wrappers (`recordTypes.brands`,
`maps.records` and `mapKinds.records`, 287 bytes) and `TypeBrand` come in
against a `makeType` 404 bytes smaller; a record's assignment function costs
more than its factory saves, and `protocol-params`' two `deftype` factories
keep their rest parameter, which a saved factory needs when the type is
redefined with more fields.

## Hash maps as tries — October 14, 2026

light-cljs-235 replaces the linear entry array of a hash map with upstream's
structure (docs/compiler.md, "Hash maps"). An array map of at most 8 keys keeps
its frozen entry array; past that, a map is a hash array mapped trie of its
entries, its nil entry beside the trie and its count, so `assoc` and `dissoc`
copy one path of nodes and lookup walks one. The trie's nodes are the ones that
light-cljs-181 already kept beside each entry array to pin upstream's traversal
order: the map now stores only the trie. Two changes made `assoc` fast as well
as logarithmic:

- **A private field, not a WeakMap entry.** Each new version of a map was
  registered in two `WeakMap`s (`maps` for its payload and brand, `mapKinds`
  for its kind). In V8 that registration, and the garbage collector's work on
  the ephemeron tables, cost more than the trie update itself: in a quick Node
  timing of the `advanced` output, building a 20,000-entry map one `assoc` at
  a time took 13.6 ms with both, against 4.3 ms once a hash map kept
  its trie in a class-private field (`#trie in value` is the brand check) and
  derived its kind from it. The field is as unforgeable as the `WeakMap`
  entry, and invisible to reflection. The wrappers that read it sit beneath
  the record wrappers of light-cljs-258 (`maps.records`, `mapKinds.records`),
  whichever installs first.
- **Trie slots hold children and entries directly**, not in `{node}` and
  `{entry}` wrappers, and a bitmap node counts its occupied slots only where
  expansion or a transient's capacity needs the count.

Transients edit the same trie as upstream's do: `transient` of a hash map
shares its root in O(1), and `assoc!`/`dissoc!` copy a node of the persistent
map the first time an edit passes through it and then change that copy, which
no other version shares, in place. Building a map with `assoc!` and removing
every second key with `dissoc!`, `(count (persistent! ...))` at `advanced` in
Node (median of 30 calls, load about 7), took 10.3 ms at n = 20,000 when every
edit copied its path and takes 6.2 ms in place (0.51 ms at 2,000, 61 ms at
200,000). Only a program that builds a hash map keeps the trie units: `lookup`
and `copyEntries` take their trie branches through wrappers that the hash map
type's effects install, and `array-map` builds its map without that type.

Command:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only map-assoc,map-get,map-assoc-existing --output DIR
```

Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM, 24 CPUs, 15 timed rounds, on
`fd989af` with this change. Another worker was building and testing on the
machine: the one-minute load average was 11.97–19.75 (median 18.63). Median ms
per call; "before" is light-cljs in "Collection throughput against upstream"
above (`49e64cc`; master's maps were still linear entry arrays), whose
upstream column is within 15% of this run's. Growth is the time at
n = 20,000 over the time at n = 2,000, light-cljs / upstream:

| Workload | n | Before | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream | Growth |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| map-assoc | 2,000 | 56.4 | 0.644 | 0.309 | 2.1× | |
| map-assoc | 20,000 | 5,643 | 6.85 | 3.49 | 2.0× | 10.6× / 11.3× |
| map-get | 2,000 | 0.629 | 0.105 | 0.055 | 1.9× | |
| map-get | 20,000 | 36.9 | 1.23 | 0.542 | 2.3× | 11.7× / 9.9× |
| map-assoc-existing | 2,000 | 50.8 | 0.320 | 0.115 | 2.8× | |
| map-assoc-existing | 20,000 | 608 | 0.351 | 0.147 | 2.4× | 1.1× / 1.3× |

Building a 20,000-entry map is about 800 times faster and now linear in n
(11× for tenfold n, as upstream's); `get` grows with n about as upstream's
does, and an `assoc` onto a 20,000-entry map costs 0.35 µs instead of 0.6 ms.
All three are within 2.8× of upstream. A run of the same persistent paths
before the transient change, at a load median of 5.29, measured 1.9× for
map-assoc (6.21 / 3.27 ms) and 2.4× for map-get (1.28 / 0.524 ms) at
n = 20,000, with growth 10.3× and 12.0×.

Sizes (bytes, size ratchet). Every `none` and `prepare` output embeds the
whole runtime, comments included, which grew 6,538 bytes (418,544 to 425,082
for `console-log-sum`); those ceilings rose to the new sizes rounded up, growth
the user accepted for light-cljs-234 to 236. At `advanced`, programs that build
no hash map are unchanged apart from `mapv-count`, which keeps the transient
lookup's trie branch (31,132 to 31,215). A program that builds one carries the
trie and its brand: `constant-keys`, whose `zipmap` builds a 36-entry hash map,
grew from 41,308 to 42,364 bytes (ceiling 42,000 to 43,000), of which the
private-field brand is 434, the trie wrappers of `lookup` and `copyEntries` 190,
and the payload builder (`hashTrie`) and lookup (`trieEntry`) 192. A map
literal of at most 8 keys (`map-only`, 9,909 bytes) keeps no trie.

## Transient hash maps — October 14, 2026

light-cljs-249 adds `map-into`, `(into {} (map (fn [i] [i i])) (range n))`, to
the `--levels` collection workloads: building a hash map through a transient,
as `into` does in both compilers. light-cljs-235 had already made a transient
edit in place the trie nodes it owns; what remained was the work around each
edit:

- **Node counts.** A bitmap or array node keeps its count of occupied slots,
  so an insertion no longer counts the node's 32 slots for its capacity, and
  every node has one shape. A new node's slot array is allocated at its length.
- **No dispatch per pair.** A transient map's `conj!` of a map entry or pair
  calls its own `assoc!`, as upstream's TransientHashMap does, unless a protocol
  implementation could answer for it; `into` with a transducer reduces through
  a fixed-arity step and `conjBangOwn` rather than the variadic `conjBang`
  protocol dispatch. `dissoc!` updates the transient's payload in place.

The same change makes a bitmap node's capacity after a persistent removal, and
after a transient packs an array node, upstream's, and expands an owned node
into new slots (docs/compiler.md, "Hash maps"); those change when a node is
rehashed, not the time measured here.

Command:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only map-into,map-assoc,map-get,map-assoc-existing --output DIR
```

Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM, 24 CPUs, 15 timed rounds, on
`ae574f4` (this change on `39f7609`). Another worker was active on the machine:
the one-minute load average was 4.31–5.31 (median 4.88). "Before" is the same
command with `--only map-into,map-assoc` on `373ead3` (light-cljs-235), at a
load median of 10.65, whose upstream column is within 16% of this run's.
Median ms per call; growth is the time at n = 20,000 over the time at
n = 2,000, light-cljs / upstream:

| Workload | n | Before | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream | Growth |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| map-into | 2,000 | 0.719 (3.3×) | 0.480 | 0.200 | 2.4× | |
| map-into | 20,000 | 7.56 (3.5×) | 4.42 | 1.89 | 2.3× | 9.2× / 9.4× |
| map-assoc | 2,000 | 0.708 (2.1×) | 0.582 | 0.292 | 2.0× | |
| map-assoc | 20,000 | 7.29 (1.9×) | 6.38 | 3.31 | 1.9× | 11.0× / 11.3× |
| map-get | 2,000 | | 0.109 | 0.054 | 2.0× | |
| map-get | 20,000 | | 1.23 | 0.546 | 2.2× | 11.3× / 10.1× |
| map-assoc-existing | 2,000 | | 0.307 | 0.114 | 2.7× | |
| map-assoc-existing | 20,000 | | 0.309 | 0.133 | 2.3× | 1.0× / 1.2× |

Building a 20,000-entry map through `into` went from 3.5× upstream to 2.3×,
and from slower than 20,000 persistent `assoc`s to a third faster; it grows
linearly, as upstream's. In a profile of the `advanced` output before the
change, counting slots and the two protocol dispatches per pair were about a
fifth of the call; what remains is mostly the `[i i]` vectors, the reduction
over `range` and the garbage collector. The persistent workloads keep their
ratios to upstream.

Sizes (bytes, size ratchet): the runtime that every `none` and `prepare`
output embeds grew 1,202 bytes, mostly comments (`console-log-sum` 425,082 to
426,284); those ceilings rose to the new sizes rounded up. At `advanced`,
`constant-keys` grew 110 bytes (42,364 to 42,474, under its 43,000 ceiling) and
`mapv-count`, whose `into` no longer keeps `conjBang`, shrank 80 (31,215 to
31,135); no `advanced` ceiling moved.

## Hash sets as tries — October 14, 2026

light-cljs-236 gives a hash set what light-cljs-235 gave a hash map
(docs/compiler.md, "Hash sets"). A set of at most 8 members keeps its frozen
member array; at its ninth distinct member it becomes, as upstream's
PersistentHashSet over a PersistentHashMap, the hash map's trie of its members,
its nil member and its count, so `conj` and `disj` copy one path of nodes and
lookup walks one. Before, a hash set was an entry array with a trie beside it
that found an entry and then searched the array for its index, and every update
copied the array. Promotion adds the members under one edit token, as upstream's
transient array map does, so traversal order and the calls that hash members
are unchanged. The set keeps its trie in a class-private field of its own,
installed by the helper that now also installs the hash map's
(`trieBrand(maps, mapKinds)`, `trieBrand(sets, setKinds)`). A transient shares
the set's trie in O(1) and edits the nodes it owns in place, as a transient hash
map does, and `conj` with one member onto a set skips its variadic loop, as it
already did for vectors. Set equality looks up each member of one set in the
other, in the order `seq` walks them, as upstream's `reduce-kv` over its map.

Command:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only set-conj,map-assoc --output DIR
```

Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM, 24 CPUs, 15 timed rounds, on
`60f7bcf` (this change on `129064e`). Another worker was active on the machine:
the one-minute load average was 8.83–10.13 (median 9.80). "Before" is
`--only set-conj` on `129064e` at a load median of 8.89, whose upstream column
is within 11% of this run's. Median ms per call; growth is the time at
n = 20,000 over the time at n = 2,000, light-cljs / upstream:

| Workload | n | Before | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream | Growth |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| set-conj | 2,000 | 55.2 (170×) | 0.479 | 0.304 | 1.6× | |
| set-conj | 20,000 | 5,716 (1,550×) | 5.18 | 3.34 | 1.5× | 10.8× / 11.0× |
| map-assoc | 2,000 | | 0.572 | 0.289 | 2.0× | |
| map-assoc | 20,000 | | 6.29 | 3.28 | 1.9× | 11.0× / 11.4× |

Building a 20,000-member set one `conj` at a time is about 1,100 times faster,
linear in n as upstream's, and 1.5× upstream; map-assoc, run alongside, keeps
its ratio. A quick timing of other set operations at `advanced` in Node (a
`setops` namespace compiled against each runtime, median of 3 calls at
n = 20,000 after one untimed, load 5–10), before / after, ms: `disj` of every
member 5,046 / 6.90; `contains?` of every member 39.2 / 2.17; `=` of two sets
built differently 45.9 / 1.41; `clojure.set/union` of two half-overlapping sets
13,754 / 14.1, `intersection` 3,864 / 14.6, `difference` 7,565 / 20.4 and
`subset?` 49.2 / 10.8; `into #{}` 2.46 / 1.97, which went through a transient
whose entry array already grew in place. `set` of a vector, 15.2 / 14.0, is
bound by walking the vector's seq.

Sizes (bytes, size ratchet): the runtime that every `none` and `prepare`
output embeds grew 3,204 bytes, mostly comments (`console-log-sum` 426,284 to
429,488); those ceilings rose to the new sizes rounded up, growth the user
accepted for light-cljs-234 to 236. At `advanced`, `set-only` (an array set of
3 constants) is unchanged at 5,214. `constant-keys`, whose dynamic two-member
set literals keep the set builder and so the trie's units, grew 277 (42,474 to
42,751, under its 43,000 ceiling), mostly the lookup wrapper for a set's trie
(111) and `putMember`: its brand is one call of the `trieBrand` that the hash
map's brand already keeps, where a second copy of the brand would have cost
about 430. `mapv-count` grew 137 (31,135 to 31,272), the one-member set path
of `conj`. Every other `advanced` output that compares values shrank 10
bytes, because equality passes a set rather than its payload; `keyword-only`
(3,497) and `map-only` (9,899) lowered their ceilings to 3,500 and 9,900.

## Seq payloads in private fields — October 15, 2026

light-cljs-259 stores the payloads of the seq kinds, cons cells (`cells`),
views (`views`), lazy seqs (`lazyValues`) and chunks (`chunks`), in
class-private fields, as vectors already did, instead of `WeakMap` entries
(docs/compiler.md, "Lists, vectors and seqs"). `privateStore()` returns a class
whose static `has`, `get` and `set` stand in for the `WeakMap`'s, so the
generic operations and the kind tables are unchanged. `LazySeq` and
`ChunkedCons` no longer store nil metadata; a ChunkedCons cell is an instance
of a class with `head`, `tail` and `hash` getters instead of a frozen literal
with three getter closures; `cons` builds its cell without a spread; and
`lazySequence` and `chunkCons` call their constructors without `new`. One
`seq-pipeline` call (n = 200) wrote 1,854 `WeakMap` entries before and writes
none now, which tests/support/js_runtime.test.js asserts.

Command:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only seq-pipeline,keyword-lookups,event-maps,vector-conj,subvec-reduce,vector-into --output DIR
```

Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM, 24 CPUs, 15 timed rounds,
another worker and this pellet's differential tests running: "before" on
master `b8c199a` (one-minute load median 7.75, range 5.19–8.38), "after" on
this change (median 7.61, range 7.16–9.01), upstream from each run. Median
µs per call:

| Workload | n | Engine | Level | Before | After | Upstream `:advanced` (after run) |
| --- | ---: | --- | --- | ---: | ---: | ---: |
| seq-pipeline | 200 | Node | `advanced` | 1,264 (17×) | 330 (3.9×) | 84 |
| seq-pipeline | 200 | Node | `none` | 5,000 | 3,771 | |
| seq-pipeline | 200 | Node | `prepare` | 4,896 | 3,750 | |
| seq-pipeline | 200 | QuickJS | `advanced` | 2,657 | 2,928 | |
| seq-pipeline | 200 | QuickJS | `none` | 22,967 | 23,650 | |
| seq-pipeline | 200 | QuickJS | `prepare` | 22,929 | 23,543 | |

Between the two runs the other workloads moved by up to 6% either way
(`keyword-lookups` at `advanced` 3,802 to 4,001 in Node), except
`subvec-reduce` at n = 2,000 (29 to 37, with minimums of 24.2 and 24.8).
Interleaved reruns, the median of seven fresh Node processes per runtime, put
the two runtimes within 3% of each other on `keyword-lookups`, `event-maps`,
`vector-conj`, `vector-into` and `subvec-reduce`, and in QuickJS within 2% on
`keyword-lookups` at `none` and `advanced`, the new runtime the faster, so
those differences are load.

In Node, `seq-pipeline` at `advanced` is 3.8 times as fast and 3.9× upstream,
not the 2× light-cljs-259 aimed for. A CPU profile of 5,000 warm calls shows
what remains: garbage collection fell from 42% of the time to 3%, and the
constructors (`LazySeq`, `ChunkedCons`, `cons`, `arrayChunk`) from about 30%
to under 10%. About 45% is now generic dispatch, `seq`, `rest`, `first`,
`next`, `count` and `nth` probing one store after another (and `vectors.has`
from each), which light-cljs-237 replaces with one slot per kind; the
pipeline's own thunks take about a fifth.

In QuickJS the change costs 10% at `advanced` and 3% at `none` and
`prepare`. QuickJS frees short-lived values by reference counting, so a
`WeakMap` entry costs it little (about 50 ns per `set` in a microbenchmark),
while adding a private field constructs a class (about 200 ns), and a store's
`get` is a call of a JavaScript function rather than of a native one. Testing
the field in `get` itself, instead of calling `has`, took the cost from 18%
to 10%; the same edit is neutral in Node.

Two layouts were measured before this one. A class per store, as `vectors`
has, was about 13% slower in Node on `seq-pipeline` than one `privateStore`
shared by the four seq stores, and 370 to 580 bytes larger at `advanced`. Moving `vectors` onto
`privateStore` as well would save another 150 bytes, but slowed `vector-conj`
by 37% and `vector-nth` by 50% at n = 20,000, as the shared methods' V8
feedback sees every store, so `vectors` keeps its own class.

Sizes (bytes, size ratchet): the runtime that every `none` and `prepare`
output embeds grew 2,092 bytes (`console-log-sum` 429,488 to 431,580), and
those ceilings rose to the new sizes rounded up to the next 1,000. At
`advanced`, `seq-pipeline` grew 456 (25,087 to 25,543): `privateStore`, four
calls of it instead of `new WeakMap()`, the ChunkedCons cell class and the
nil-metadata tests. An output that tests values against the seq stores
without building seqs keeps `privateStore` and three of its calls, 289 more
(`keyword-only` 3,497 to 3,786, `atom-str` 2,422 to 2,711). The user accepted
up to 1 KB of `advanced` growth per ceiling for this pellet's speed target;
the ceilings it passed rose to their new sizes rounded up: `rest-fn` 11,000,
`keyword-case` 3,900, `constant-lookups` 4,300, `constant-keys` 44,000,
`vector-only` 5,300, `keyword-only` 3,800, `map-only` 11,000, `atom-str`
2,800, `set-only` 5,600, `closed-params` 15,000 and `protocol-params` 44,000.
For the same reason the bound on `(count (.split "a,b" ","))` in
tests/compiler/builtin_types.rs rose from 1,600 to 1,900 bytes (it is about
1,810).

## Protocol slot and factory call shapes — October 14, 2026

light-cljs-262 changes two call shapes at `advanced`. A devirtualized protocol
call holds each argument it cannot read twice in an operand local of its
function instead of passing it to an arrow applied at each call, and a call of
a positional or map factory or of a protocol method's dispatcher that nothing
reassigns is a plain call instead of `.call(null, ...)`. The record-protocol
loop body goes from

    ((r0) => r0["cljs$user$Area$area$arity$1"](r0))((g3).call(null, l7, 2))

to `(operand0 = g3(l7, 2))["cljs$user$Area$area$arity$1"](operand0)`.

The pellet's acceptance command, on the 24-CPU Mac (Node v24.18.0, macOS
15.7), with another worker building and testing on it:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only record-protocol --output DIR
```

"Before" is master `129064e`. Milliseconds per call `(f 2000)`, median /
minimum of the mean of timed calls 11-30 in fresh processes; upstream is
timed in the same rounds:

| Build | One-minute load (median of rounds) | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream |
| --- | ---: | ---: | ---: | ---: |
| before | 3.0 | 0.038 / 0.035 | 0.019 / 0.017 | 2.0× |
| after | 5.8 | 0.048 / 0.044 | 0.019 / 0.017 | 2.5× |

QuickJS at `advanced` went from 1.52 to 1.35 ms a call, and the output from
5,475 to 5,463 bytes. The acceptance target, 1.5×, is not met, and the Node
median is worse, though the final optimized loop is five times faster.
Timing each call in fresh processes (median of 11, load 5.4) shows why:

| Program | Mean of calls 11-30 (µs) | Final µs per call | At final speed from call |
| --- | ---: | ---: | ---: |
| upstream `:advanced` | 24.9 | 10 | 5 |
| before | 40.0 | 19 | 21 |
| after | 54.4 | 4 | 28 |
| before, hand-written constructor | 38.2 | 20 | 12 |
| after, hand-written constructor | 24.4 | 10 | 8 |

Upstream's loop is `new C(b)` and two field reads: Closure inlined the
factory, the dispatcher and the method. In light-cljs the shape of the call
matters little while records are built by `makeType`'s shared constructor and
`TypeBrand` (Maglev alone, `node --no-turbofan`: 56 µs a call before, 54
after). Once TurboFan inlines the plain factory call it removes the record
allocation (4 µs a call), but its concurrent compile takes about as long in
wall time as before, and the faster calls before it mean more of them run
first; fewer allocations per call also move the second costly young
generation collection (about 300 µs) from call 9 into the timed window. The
last two rows replace the type's constructor, by hand, with a constructor
function of its own that assigns the five fields, with the method on its
prototype, the shape light-cljs-264 plans: then the new call shape is at
upstream's speed (0.98×), where the old one stays at 1.5×, and in Maglev it
halves the time a call takes (9 against 20 µs).

## Per-type output — October 15, 2026

light-cljs-264 writes each `deftype` and `defrecord` type at `advanced` as
statements of the function it is in, without a function around them, and
gives the type a constructor of its own instead of `makeType`'s one
constructor for every type (`ExprKind::DefType` in `src/emitter.rs`,
`defineType` in `src/js_runtime.js`). A record of `eight-records` goes from

    (()=>{c=a(["a","b"],[["cljs.user/Shape/area",1,(function(ak){const B=this;
    ak=this;return B.a+B.b})]],true,null,"cljs.user/R1",()=>c,function(C,D,E,F,
    G){this.a=C;this.b=D;this.__meta=E;this.__extmap=F;this.__hash=G});d=(C,D)
    =>new c(C,D,null,null,null);c.cljs$lang$type=true;return c})();

to

    g=b(function Type(E,F,G,H,I){this.a=E;this.b=F;this.__meta=G;this.__extmap=H;
    this.__hash=I;new c(this,Type)},"cljs.user/R1 a b",[[f,1,function(){return
    this.a+this.b}]],1);h=(E,F)=>new g(E,F,null,null,null);

The masks moved to the prototype, `cljs$lang$type` and a `deftype`'s
`cljs$lang$ctorStr` into `defineType`, the method entry names the protocol
method's dispatcher, the method reads `this` without upstream's dead
prologue, a record that one top-level form defines has no
`currentConstructor`, and a
positional factory nothing reads (a `deftype` built with `T.`) is not
defined.

**Per type.** `eight-records` is new: eight records and eight deftypes, each
with two fields and one method, all constructed and passed to one function
that calls the method and reads a field. Bytes at `advanced` that one more
record or deftype adds, as
`size_ratchet::each_type_of_eight_records_costs_at_most_its_budget` measures
them (raw the same for each, gzip the mean of seven); "before" is
light-cljs-262 on master `bdaa6a5` (`30a1bf7`), upstream the investigation's
`:advanced` figures:

| Per extra | Before | After | Upstream `:advanced` |
| --- | ---: | ---: | ---: |
| record (2 fields, 1 method) | 295–303 / 52 | 218 / 28 | 1,763 / 195 |
| deftype (2 fields, 1 method) | 279–287 / 56 | 130 / 20 | 135 / 33 |

The whole fixture goes from 33,689 / 10,460 to 31,803 / 9,965 bytes.

**Sizes**, bytes of `--compile --optimize LEVEL` (`cargo nextest run --test
compiler size_ratchet:: --no-capture`). Every `advanced` output with types
shrinks and none grows; the five ceilings in the last column fall. `none`
and `prepare` embed the whole runtime, which grew by 997 bytes
(`decorateType`, shared by `makeType` and `defineType`, and `defineType`),
so their ceilings rise by 1,000 bytes:

| Fixture | Advanced before | Advanced after | Ceiling after (none / prepare / advanced) |
| --- | ---: | ---: | --- |
| record-protocol | 5,463 | 5,385 | 434,000 / 434,000 / 5,400 |
| three-records | 5,456 | 5,380 | 436,000 / 436,000 / 5,400 |
| pure-defs | 5,355 | 5,294 | 435,000 / 435,000 / 5,300 |
| closed-params | 14,036 | 13,963 | 436,000 / 436,000 / 14,000 |
| callbacks | 37,905 | 37,832 | 435,000 / 435,000 / 38,000 |
| protocol-params | 43,103 | 42,740 | 436,000 / 436,000 / 43,000 |
| eight-records | 33,689 | 31,803 | 445,000 / 445,000 / 32,000 |

**Construction.** The pellet's acceptance command, on the 24-CPU Mac (Node
v24.18.0, macOS 15.7.3), with another worker building and testing on it:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only record-protocol --output DIR
```

Microseconds per call `(f 2000)` at `advanced`, median of the run's samples,
runs alternated (one-minute load 1.6 to 3.8); upstream is timed in the same
rounds:

| Build | Runs | Node | Upstream `:advanced` | light-cljs / upstream | QuickJS |
| --- | ---: | ---: | ---: | ---: | ---: |
| master `bdaa6a5` | 3 | 37.5–37.9 | 19–20 | 1.9–2.0× | 1,425–1,515 |
| light-cljs-262 (`30a1bf7`) | 2 | 48.0–49.7 | 19 | 2.5–2.6× | 1,311–1,360 |
| light-cljs-264 | 4 | 33.6–35.2 | 19–20 | 1.7–1.9× | 943–987 |

`none` and `prepare`, which keep `makeType`, stay at 270–290 µs in Node
and about 5,100 in QuickJS. Most of what is left is the brand: the
constructor ends with `new TypeBrand(this, Type)`, which adds the private
field through `TypeBrand`'s return-override base class. A Node
microbenchmark of construction alone (an escaping instance with two fields
and a record's three slots, each type its own factory and loop; ns per
construction, TurboFan / `--no-turbofan`) shows what it costs and how it
scales with the number of types:

| Shape | 1 type | 4 types | 16 types |
| --- | ---: | ---: | ---: |
| per-type constructor, no brand | 3.7 / 6.1 | 3.7 / 6.0 | 3.7 / 5.8 |
| per-type constructor and `new TypeBrand` (this change) | 3.8 / 26 | 4.1 / 28 | 14.8 / 30 |
| shared constructor, `init` and `TypeBrand` (light-cljs-258) | 4.2 / 29 | 4.4 / 31 | 22.9 / 36 |
| per-type class extending a base with the private field | 13 / 23 | 14 / 24 | 22 / 26 |

A private field is defined only by a class constructor, so every brand is
added at `TypeBrand`'s one site, which goes megamorphic past four types, and
Maglev does not inline the return override at all. Writing the record's
three slots there too would have saved 38 bytes a record but took 27 ns at
16 types, so the constructor writes them.

## Code-splitting example bounds — October 15, 2026

light-cljs-269 measures `examples/features`, the application the
[code-splitting design](../docs/code-splitting.md) is measured against
(`app.main`, `app.shared`, and the two features `app.editor` and
`app.reports`), and records the bounds under its "Goal". `bb
benchmarks/features.bb` builds the compiler and prints the table below. It
compiles the single-file `--esm` output at each level (`advanced` is terse)
of the whole application, of the application with each feature removed and
with both removed (the lower bound for the startup chunk), and of each
feature as its own entry (the upper bound for its chunk). Variants are
generated from the sources into a temporary directory: the feature's file
left out, its libspec dropped, and each form naming it replaced by `nil`.
Cells are raw / `gzip -9` bytes; the last column is the `advanced` runtime
slice as `--size-report` counts it. Master `6804bfb`, with `app.main` still
calling both features directly (before light-cljs-271); the Apple M3 Ultra
virtual machine of the [environment](results/environment.txt), macOS 15.7.3,
one-minute load average 9.2 (sizes do not depend on it).

| Program | `none` | `prepare` | `advanced` | `advanced` runtime slice |
| --- | ---: | ---: | ---: | ---: |
| `app.main`, the whole application | 452,213 / 108,808 | 452,213 / 108,808 | 72,486 / 21,732 | 67,147 |
| `app.main` without `app.editor` | 450,559 / 108,607 | 450,559 / 108,607 | 67,064 / 20,162 | 62,504 |
| `app.main` without `app.reports` | 436,315 / 106,437 | 436,192 / 106,420 | 64,870 / 20,192 | 63,791 |
| `app.main` without `app.editor` and `app.reports`: startup lower bound | 434,998 / 106,245 | 433,736 / 106,054 | 4,535 / 1,701 | 4,189 |
| `app.editor` as its own entry: chunk upper bound | 435,455 / 106,284 | 435,064 / 106,209 | 64,447 / 20,035 | 63,644 |
| `app.reports` as its own entry: chunk upper bound | 449,706 / 108,526 | 449,706 / 108,526 | 66,841 / 20,117 | 62,504 |

At `none` and `prepare` every row embeds the whole 432,063-byte runtime, so
the features are 17 KB of 452 KB. At `advanced` startup without its
features is 4,535 bytes, 6% of the application: the Phase 3 target is that
size plus under 200 bytes per `loadable`, under 4,935 bytes. Each
feature's own entry is mostly its runtime slice, 98.8% for `app.editor` and
93.5% for `app.reports`. By subtraction from the whole application's
67,147-byte slice, about 4.6 KB of runtime is needed by the editor alone,
3.4 KB by reports alone and 4.2 KB by startup; the other 55 KB is needed by
both features and not by startup, so it is the `app.editor+app.reports`
chunk, and loading the second feature costs only its own few kilobytes.
`clojure.string/join` is written with `js*`, and a program with raw `js*`
text keeps its layout and prefixed names at `advanced`: the program part
beside the runtime is 4,337 bytes for `app.reports` as its own entry against
803 for `app.editor`, and 5,339 for the whole application against 1,079
without `app.reports`.

## Resident loadables in examples/features — October 15, 2026

light-cljs-271 switched `app.main` of `examples/features` from calling
`app.editor/start` and `app.reports/start` to holding
`(lazy/loadable app.editor/start)` and `(lazy/loadable app.reports/start)` and
loading them in sequence, with both targets resident in the one file. Bytes of
the single-file output, the same binary compiling the example as it was before
the switch and after it (24-CPU Mac, macOS 15.7.3, one-minute load 2.3):

```sh
cargo run --quiet -- --compile --optimize LEVEL --entry app.main --source-root examples/features | wc -c
```

| Level | Direct calls | Two loadables | Growth |
| --- | ---: | ---: | ---: |
| `none` | 452,197 | 454,642 | +2,445 |
| `prepare` | 452,197 | 454,642 | +2,445 |
| `advanced` | 72,470 | 80,391 | +7,921 |

At `advanced`, `--size-report` puts 6,613 of the growth in the runtime slice
(455 to 503 units): `rt$defineType`, which the example's first `deftype`
(`light.lazy/Loadable`) demands, is 39 units and 6,553 bytes, and `rt$deref`
4 units and 351 bytes. The rest is `light.lazy` itself, the two sites and the
features' `start` functions, which now escape with their dispatchers instead
of being called directly. This is far above the 200 bytes per site the Goal
table targets; light-cljs-272 measures it in the ratchet and trims it.

`bb benchmarks/features.bb` (light-cljs-269) after the switch, raw / `gzip -9`
of the `--esm` output at `advanced`: the whole application 80,407 / 24,286
(runtime slice 73,760), against 72,486 / 21,732 before; without both features
5,233 / 1,923 (slice 4,695), against 4,535 / 1,701, since `start` keeps its
`lazy/load` calls when the script turns the sites into `nil`. The feature rows
are unchanged.

## Terse raw `js*` — October 15, 2026

light-cljs-294. A program with any raw `js*` text used to keep the emitter's
layout at `advanced`, with every compiler name the hygiene prefix and a short
name (`__light0_a`); `clojure.string/join` is `js*`, so every program calling
it lost terse emission. `terse::terse_program` now lexes raw text with the
program, writes each of its tokens and comments as they are, and gives the
compiler's names short names that avoid every identifier the raw text
contains. Only a program whose raw text puts a hole inside one of its strings
or comments, or that the lexer may read otherwise than an engine (a byte
outside ASCII outside string literals, an HTML-like comment, a `/` after
`}`), keeps the old layout. Measured with `bb benchmarks/features.bb`
on master `b1c15d9` (after light-cljs-271, resident loadables) and on this
change, the same sources; 24-CPU Mac, macOS 15.7.3, one-minute load 5.3 and
5.6 (sizes do not depend on it). Raw / `gzip -9` bytes of the single-file
`--esm` output at `advanced`; "program" is the output beside its runtime
slice, which does not change:

| Program | Before | After | Program before | Program after |
| --- | ---: | ---: | ---: | ---: |
| `app.main`, the whole application | 80,407 / 24,286 | 76,145 / 23,900 | 6,647 | 2,385 |
| `app.main` without `app.editor` | 74,481 / 22,577 | 70,816 / 22,215 | 5,768 | 2,103 |
| `app.main` without `app.reports` | 73,289 / 23,006 | 73,289 / 23,006 | 1,667 | 1,667 |
| `app.main` without both features | 5,233 / 1,923 | 5,233 / 1,923 | 538 | 538 |
| `app.editor` as its own entry | 64,447 / 20,035 | 64,447 / 20,035 | 803 | 803 |
| `app.reports` as its own entry | 66,841 / 20,117 | 63,927 / 19,817 | 4,337 | 1,423 |

The script now also names the rows whose `advanced` output carries the
prefix: before, the whole application, the application without `app.editor`
and `app.reports` as its own entry; after, none. In the size ratchet only
`raw-js` has raw text: its `advanced` output goes from 36 to 26 bytes,
`(()=>{void 0;return 3})()`, under the unchanged 100-byte ceiling. The
`none` and `prepare` columns do not change.

## Loadable stubs — October 15, 2026

light-cljs-272 measures what a `light.lazy/loadable` site costs at
`advanced` and trims it. light-cljs-271 wrote `light.lazy/Loadable` as a
`deftype`, and any program type brings an `advanced` program the type
machinery (`rt$defineType`, 39 units and 6,553 bytes in `examples/features`,
most of it the protocol hooks every core operation needs once a program type
may implement its protocol). The runtime now owns the type: a class with
`deref()` and `load()`, 254 bytes terse, plus 77 bytes for its printed names
where the program prints; its IDeref implementation as a `defineType` gives
it, and the dispatch to one `extend-type` or `specify!` gives, are added only
where the program has types of its own or looks protocols up
(docs/code-splitting.md, Phase 1, "As built (light-cljs-272)"). Sizes, which
do not depend on load, on the 24-CPU Mac of the
[environment](results/environment.txt), macOS 15.7.3, one-minute load
average 3.1, on master `965135d` (after light-cljs-294, which writes
`clojure.string/join`'s `js*` tersely) and on this change:

```sh
cargo nextest run --test compiler size_ratchet:: --no-capture
bb benchmarks/features.bb
cargo run --quiet -- --compile --optimize advanced --size-report --entry app.main --source-root DIR --esm | wc -c
```

`examples/features` with its two loadables, single-file `--esm`, raw /
`gzip -9`:

| `advanced` | light-cljs-271 (`deftype`), master `965135d` | light-cljs-272 (the runtime's type) |
| --- | ---: | ---: |
| `app.main`, the whole application | 76,145 / 23,900 | 69,500 / 21,646 |
| its runtime slice | 73,760 | 67,482 |
| without both features: startup lower bound | 5,233 / 1,923 | 4,680 / 1,753 |

What a site costs, as the difference between a program and the same program
reading its targets directly (raw bytes at `advanced`). The ratchet rows are
`emit_js` output; the others single-file `--esm`. `features-direct` is
`examples/features` with each `(lazy/loadable app.editor/start)` replaced by
`app.editor/start`, its namespace required, and each `(lazy/load ...)` by
`(js/Promise.resolve ...)`: the same example with direct calls, which the
Phase 1 exit criterion compares against. "Before light-cljs-271" is the
example as it was then, calling both features from `start`:

| Comparison | Loadables | Direct | Difference | Per site |
| --- | ---: | ---: | ---: | ---: |
| ratchet `loadable-site` against `direct-site`, one site | 587 | 280 | 307 | 307 |
| ratchet `features` against `features-direct` | 69,484 | 69,092 | 392 | 196 |
| `examples/features` against the same with direct calls, `--esm` | 69,500 | 69,108 | 392 | 196 |
| `examples/features` against before light-cljs-271, `--esm` | 69,500 | 68,904 | 596 | 298 |

One site carries the whole type: the runtime factory and class are 286 of
`loadable-site`'s 307 bytes. Two sites share it, so the stubs of
`examples/features` cost 196 bytes a site, under the 200 of the Phase 1 exit
criterion, which
`size_ratchet::features_loadable_sites_cost_under_200_bytes_each_at_advanced`
asserts. Of the 392 bytes, 335 are the runtime slice's (67,482 against
67,147: the class, its printed names and its slot in the runtime factory);
the program's other 57 are `light.lazy/Loadable`, `light.lazy/load` (a
method call the inliner keeps as a function, since it is called twice and a
method call has an effect) and the two `new` expressions, less the two
`Promise.resolve` reads of the direct program. The class calls its getter
as the `deftype` did, `getter.call(null)`, so that an IFn object serves as
one (`lazy::loadable_getters_keep_cljs_invocation_semantics_at_every_level`):
9 of the bytes, 192 a site before. Against the example before light-cljs-271
the growth is 596 bytes: the loadable targets escape as functions where that
`start` inlined both features, and `start` chains two promises.

New ratchet rows (`NAMESPACE_FIXTURES`, programs of several namespaces), bytes
/ ceiling:

| Fixture | `none` | `prepare` | `advanced` |
| --- | ---: | ---: | ---: |
| `features` | 455,093 / 456,000 | 455,093 / 456,000 | 69,484 / 70,000 |
| `features-direct` | 453,989 / 454,000 | 453,989 / 454,000 | 69,092 / 70,000 |
| `loadable-site` | 435,647 / 436,000 | 435,517 / 436,000 | 587 / 600 |
| `direct-site` | 434,864 / 435,000 | 434,864 / 435,000 | 280 / 300 |

With light-cljs-271's `deftype`, `loadable-site` was 6,600 bytes at
`advanced`. The `advanced` size of every other fixture is unchanged. `none`
and `prepare` embed the whole runtime, which the `Loadable` units make 1,530
bytes longer in every fixture, so their ceilings rise to the new size rounded
up: by 2,000 bytes for 19 fixtures and 1,000 for 11, and one keeps its
ceiling.

## Direct reads of Loadable's decorations — October 15, 2026

light-cljs-296 keeps what the runtime gives light.lazy's `Loadable` as a
`deftype` where a program reads it by name rather than through an operation
that observes it: `(.cljs$core$IDeref$_deref$arity$1 l l)` threw a
`TypeError` at `advanced` and `(.-cljs$lang$ctorStr lazy/Loadable)` was
`undefined`. Rapid type analysis now roots a unit of the `when:` list of the
effect adding the name (`LOADABLE_MEMBERS` in `src/runtime_slice.rs`) in a
program that keeps `Loadable`. Sizes, which do not depend on load, on the
24-CPU Mac of the [environment](results/environment.txt), macOS 15.7.3,
one-minute load average 9.9, of
`(let [L (light.lazy/loadable-type) l (new L nil (fn [] "value"))] (js/console.log READ))`
with each `READ`, raw bytes at `advanced`, on master `d7dc698` and on this
change:

```sh
cargo run --quiet -- --compile --optimize advanced --text '<program>' | wc -c
```

| `READ` | master `d7dc698` | light-cljs-296 |
| --- | ---: | ---: |
| `l` (no decoration read) | 502 | 502 |
| `(.cljs$core$IDeref$_deref$arity$1 l l)` | 537, throws | 5,707 |
| `(.-cljs$core$IDeref$ l)` | 520, `undefined` | 5,690 |
| `(.-cljs$lang$protocol_mask$partition0$ l)` | 5,744 | 5,744 |
| `(.-cljs$lang$ctorStr L)` | 520, `undefined` | 615 |
| `(.-cljs$lang$type L)` | 517, `undefined` | 612 |

The marker and slot need `Loadable.type`, which brings `defineType` and the
protocol machinery, as any program type does; a mask already kept them,
since naming a mask roots `installMethods`. The names cost `Loadable.name`
and `typeNames`, 95 bytes. No ratchet fixture names these, so no size
changes at any level.

## Chunked output of examples/features — October 15, 2026

light-cljs-274 writes one ES module per chunk of the namespace-level plan
(`PreparedApplication::emit_chunks`; [code-splitting design](../docs/code-splitting.md),
Phase 2, "As built (light-cljs-274)"). There is no CLI for it until
light-cljs-275, so the files are those `emit_chunks` returns for
`examples/features` at each level, as
`chunks::the_features_example_runs_as_three_chunks_at_every_level` builds
them, written out and measured with `wc -c` and `gzip -9`; the single file
is `emit_esm`, which the change leaves byte for byte as it was. Branch of
light-cljs-274 on master `ba3d120`; the Apple M3 Ultra virtual machine of the
[environment](results/environment.txt), macOS 15.7.3, one-minute load average
2.9 (sizes do not depend on it). Raw / `gzip -9` bytes:

| File | `none` | `prepare` | `advanced` |
| --- | ---: | ---: | ---: |
| single file, `app.main` `--esm` | 455,109 / 109,611 | 455,109 / 109,611 | 69,500 / 21,646 |
| `app.main.js`, startup | 441,976 / 107,628 | 441,976 / 107,628 | 69,587 / 21,575 |
| `app.editor.js` | 1,295 / 423 | 1,295 / 423 | 276 / 224 |
| `app.reports.js` | 13,101 / 2,612 | 13,101 / 2,612 | 832 / 517 |

Startup keeps the runtime (the whole runtime below `advanced`, the
whole-program slice at `advanced`) and every constant, as Phase 2 plans, so
at `advanced` it is 87 bytes over the single file: the features' forms move
out, and startup gains its `export` lines, the two `lazy$` loadables and
`lazy$` itself, which records loaded chunks and whose local names terse
output keeps (686 bytes). Each feature is its forms and its imports of startup's runtime
bindings, constants and helpers. The features' runtime units and constants
leave startup in Phase 3. `app.reports.js` at `none` is mostly
`clojure.string`, which only reports requires.

## Definition-level chunk placement — October 15, 2026

light-cljs-277 moves the definitions that removal could drop, and
`defprotocol` and `deftype` forms, toward the chunks of the roots whose code
reads them at `advanced` ([code-splitting design](../docs/code-splitting.md),
Phase 3, "As built (light-cljs-277)"). Measured with
`light-cljs --compile --optimize advanced --entry app.main --source-root
examples/features --out-dir DIR`, `wc -c` and `gzip -9`, against the same
command built from master `ed49b92` (namespace-level placement,
light-cljs-274/275); `none` and `prepare`, where nothing moves, and the
single `--esm` file are unchanged, and `bb benchmarks/features.bb` prints the
bounds of the Goal table as before. Apple M3 Ultra virtual machine of the
[environment](results/environment.txt), macOS 15.7.3, one-minute load average
1.8 (sizes do not depend on it). Raw / `gzip -9` bytes at `advanced`, terse:

| File | Namespace level | Definition level |
| --- | ---: | ---: |
| `app.main.js`, startup | 69,587 / 21,575 | 69,539 / 21,551 |
| `app.editor+app.reports.js` | none | 63 / 81 |
| `app.editor.js` | 276 / 224 | 361 / 255 |
| `app.reports.js` | 832 / 517 | 874 / 531 |

The shared definitions only the features read are `app.shared/documents`,
which both read, now the whole `app.editor+app.reports.js`, and
`app.shared/index-by`, which only the editor reads (`pluck` is inlined into
`app.reports`). In the namespace-level startup chunk their statements are 48
bytes, `(N=aq);` and `(au=function(av,Q){return k(l(av,Q),Q)});`, and startup
shrinks by those 48 bytes. Their two names left startup's `export let` line
(5 bytes), and startup exports two names to the moved code instead (5
bytes): the constant table entry `documents` holds and the runtime's
`zipmap`. Those go with constants (light-cljs-278) and runtime units
(light-cljs-279), which still all sit in startup. `label`, which the menu
reads, stays.

## Watch step of examples/dev — October 15, 2026

light-cljs-285 adds `--watch` ([Watch](../docs/evaluation.md#watch);
[dev-workflow design](../docs/dev-workflow.md), Phase 2). The step time is
what the watch prints for each step, from the poll that noticed a save to
the unit, its map and the manifest written, and what
`dev_watch::a_saved_change_rewrites_the_unit_the_running_build_reloads`
reads and asserts (best of five saves under 20 ms with `--release`, under
100 ms in the debug build): five saves of
`app/view.cljs` per run, three runs, with
`cargo nextest run --release --test dev_watch --no-capture a_saved_change`
(release CLI) and the same without `--release` (the debug test binary, which
`cargo nextest run` uses). Branch of light-cljs-285 on master `b5e1dd2`; the
Apple M3 Ultra virtual machine of the [environment](results/environment.txt),
macOS 15.7.3, one-minute load average 3.5-9.7 with a peer worker building.

| Measurement | Before | After |
| --- | ---: | ---: |
| A saved `app.view` to its unit on disk, release CLI | no watch path: the whole dev build, 246 ms | 3.9-4.6 ms (15 saves, median 4.3) |
| The same, debug test binary | — | 8.9-11.1 ms |
| `--watch` startup: the dev build, then the session's load of the program | — | 279-290 ms |
| `app.view.js` after the save | 3,492 bytes (map 974), the build's unit | 9,385 bytes (map 646) |

The rewritten unit is larger because the session's unit of each of the
namespace's four forms registers the reader context of every namespace the
session knows, which the build's unit does once for its own (light-cljs-301
is to register only the unit's own).

## Constants placed by their readers — October 15, 2026

light-cljs-278 splits the `advanced` constant table across the chunks of a
chunked build: each entry is declared once, in the chunk of the roots whose
code reads it, directly or through a collection holding it
([code-splitting design](../docs/code-splitting.md), Phase 3, "As built
(light-cljs-278)"). Measured with `light-cljs --compile --optimize LEVEL
--entry app.main --source-root examples/features --out-dir DIR`, `wc -c` and
`gzip -9 -n`, against the same command built from master `6c98a81`
(light-cljs-277). `none` and `prepare`, which write no constant table, are
byte for byte as before, as is the single `--esm` file, and
`bb benchmarks/features.bb` prints the bounds it printed before (startup
lower bound 4,680 / 1,753). Apple M3 Ultra virtual machine of the
[environment](results/environment.txt), macOS 15.7.3, one-minute load average
11.5 from peers' builds (sizes do not depend on it). Raw / `gzip -9` bytes at
`advanced`, terse:

| File | Before | After |
| --- | ---: | ---: |
| `app.main.js`, startup | 69,539 / 21,551 | 69,000 / 21,242 |
| `app.editor+app.reports.js` | 63 / 81 | 632 / 397 |
| `app.editor.js` | 361 / 255 | 361 / 258 |
| `app.reports.js` | 874 / 531 | 874 / 531 |

Startup shrinks by 539 bytes, 309 gzipped: the table of
`app.shared/documents`, eleven keywords, four sets, four maps (with the four
title strings) and the vector, which only the features read, now sits with
`documents` in their shared chunk, and both features import the keywords
they read (`:id`, `:title`, `:words`, `:tags`, `:draft`) from there instead
of from startup. Startup keeps `:editor` and `:reports`, which its menu
reads. The features' files keep their sizes: their import lines name
the same number of bindings, split between startup and the shared chunk.
What remains of startup beyond the lower bound is the whole-program runtime
slice, which light-cljs-279 splits.

## Chunks that hold no code — October 15, 2026

light-cljs-299 drops, at `advanced`, a chunk that would write no code: one
that holds no root and no Var another chunk reads, whose namespaces' forms
all emit nothing there because removal, inlining or definition-level
placement left them nothing ([code-splitting design](../docs/code-splitting.md),
Phase 3, "As built (light-cljs-299)"). Measured with `light-cljs --compile
--optimize LEVEL --entry app.main --source-root DIR --out-dir OUT`, `wc -c`
and `gzip -9` of standard input, against the same tree with the drop
disabled, which writes what master `684cbb8` (light-cljs-278) writes. `none`
and `prepare` are byte for byte as before, and so is every file of
examples/features at every level, whose shared chunk holds
`app.shared/documents`. Apple M3 Ultra virtual machine of the
[environment](results/environment.txt), macOS 15.7.3, one-minute load average
2.7 (sizes do not depend on it). The `diamond` fixture
(tests/fixtures/chunks/diamond), raw / `gzip -9` bytes at `advanced`, terse:

| File | Before | After |
| --- | ---: | ---: |
| `app.main.js`, startup | 23,593 / 7,998 | 23,593 / 7,998 |
| `app.editor+app.reports.js` | 12 / 32 | none |
| `app.editor.js` | 143 / 132 | 105 / 115 |
| `app.reports.js` | 153 / 139 | 115 / 121 |

`app.format/line` and `app.text/shout` are inlined into both features, so
the shared chunk was `(()=>{})();`. Each feature loses its
`import "./app.editor+app.reports.js";` line, 38 bytes, and loading one
feature takes two requests instead of three. Startup is unchanged; its
manifest entry lists `app.text` and `app.format`, whose forms write nothing.

## Namespace registration in session and watch units — October 15, 2026

light-cljs-301: a session's unit registers with the runtime only the
namespaces whose reader context or definitions changed since the session's
last unit (all of them in its first), instead of every namespace the
session knows; the form units of a watch step register none, and the unit
the step rewrites registers its namespace once, with the definitions of all
its forms, and runs the forms' statements under one prelude instead of one
per form ([Watch](../docs/evaluation.md#watch); "The dev build" in
[dev-workflow.md](../docs/dev-workflow.md)). Same method as "Watch step of
examples/dev" above: the step time the watch prints for five saves of
`app/view.cljs` per run in
`dev_watch::a_saved_change_rewrites_the_unit_the_running_build_reloads`,
`cargo nextest run --release --test dev_watch --no-capture a_saved_change`
four times (release CLI) and the same without `--release` three times (debug
test binary). Branch of light-cljs-301 on master `b811387`; the Apple M3
Ultra virtual machine of the [environment](results/environment.txt), macOS
15.7.3, one-minute load average 11.7-12.3 with peer workers building. The
Before column is light-cljs-285's run above, at load 3.5-9.7.

| Measurement | Before | After |
| --- | ---: | ---: |
| `app.view.js` after the save | 9,385 bytes (map 646) | 3,565 bytes (map 542) |
| Namespace registrations in it | 16: 4 forms × 4 namespaces | 1 |
| The build's `app.view.js`, for comparison | 3,492 bytes (map 974) | 3,492 bytes (map 974) |
| A saved `app.view` to its unit on disk, release CLI | 3.9-4.6 ms (15 saves, median 4.3) | 4.0-4.6 ms (20 saves, median 4.3) |
| The same, debug test binary | 8.9-11.1 ms | 8.7-9.4 ms (15 saves) |
| `--watch` startup, release CLI | 279-290 ms | 304-313 ms |

The rewritten unit is now 2% larger than the build's, for the four forms'
own scopes; the step time of this four-namespace example is within the
noise of the two loads, since its registrations were a small part of the
step. What changed is how they grow: a step's unit carried forms ×
namespaces registrations, each name of each namespace spelled as `\uXXXX`
escapes, and now carries its namespace's once, so the unit of a namespace
in an application of many namespaces no longer grows with their number
(the first unit in load order still registers every namespace, as the
build's first unit does). A REPL form's unit compares the session's
namespaces with what it last registered, which costs a pass over the
definitions instead of writing every namespace's registration, and a watch
step's form units skip even that.

## Save to updated page of examples/dev — October 15, 2026

light-cljs-287 adds the reload policy to `--watch --serve`: each step loads
its reload set (the namespaces it wrote, `^:dev/always` ones, never
`^:dev/once` ones, with `--reload-dependents` those downstream), finds the
hooks again in the retained program, and sends the hooks and the namespace
policy with each load ([Serve](../docs/evaluation.md#serve); "The reload set"
in [dev-workflow.md](../docs/dev-workflow.md)). The Goal row "save to updated
page", measured as light-cljs-286 measured it: a copy of examples/dev served
by the debug CLI, `light-cljs --watch --entry app.main --source-root SRC
--out-dir DEV --poll-ms 20 --serve 0` (with `--reload-dependents` for the last
row), the built-in page open in headless Google Chrome driven over the
DevTools protocol by a throwaway Node script, which saved the source by a
rename and timed the save to the page's console event of the new render
line. Branch of light-cljs-287 on master `c4f357f`; the Apple M3 Ultra
virtual machine of the [environment](results/environment.txt), macOS 15.7.3,
one-minute load average 2.1-3.1 with a peer worker building. The Before
column is light-cljs-286's run, at load 1.9.

| Measurement | Before | After |
| --- | ---: | ---: |
| A saved `app.view` to its new render on the page | 29-39 ms (5 saves) | 26.6-66.3 ms (14 saves, median 30.8) |
| The step the watch prints for it (debug CLI) | — | 8.5-37.2 ms |
| The client's load of the pushed unit, between the hooks | about 1 ms | 0-2 ms |
| A saved `app.state` to the new render, `--reload-dependents` (loads `app.state`, `app.view`, `app.main`) | — | 18.1-40.5 ms (10 saves; steps 3.2-16.8 ms) |

The policy's own work in a step is a set over the manifest's namespaces and
a pass over the retained program's definitions for the hooks; the spread of
the page times follows the debug CLI's step times under the peer's load, and
every save stayed under the 100 ms target.

## Unread type brands — October 18, 2026

light-cljs-262 finishes with the brand rather than the call shape. After
light-cljs-264 the record-protocol loop body was already
`(operand0 = g3(l7, 2))["cljs$user$Area$area$arity$1"](operand0)`, with no
applied arrow and no `.call(null, ...)`, and the workload was still 1.8-1.9×
upstream. Timing each of the first 40 calls in fresh Node processes (median
of 9 to 11) showed where the timed calls 11-30 went: in V8's mid tier
(Maglev) a construction took about 30 ns against 10 without
`new TypeBrand(this, Type)`, whose return-override constructor and private
field Maglev does not inline, and the extra allocation brought a young
generation collection (about 300-400 µs) into the window sooner. Hand-edited
variants of the output, each call timed on its own, median over 9 to 11
fresh processes, mean of calls 11-30 (load 3.5 to 4; upstream gives 8.8 µs
this way, where its first costly collection falls on call 10, and 19 in the
benchmark driver, where it falls in the window):

| Program | Mean of calls 11-30 (µs) |
| --- | ---: |
| light-cljs `advanced`, master `fb3b6e4` | 35.9-44.2 |
| factory call inlined (`new c(l, 2, null, null, null)`) | 43.7 |
| type as a class with a private field (`#t = 1`) | 31.4 |
| type as a class with a private method brand | 33.5 |
| no brand | 22.8-24.5 |

A `WeakSet.add(this)` brand took 345 µs a call in the driver.

Nothing in record-protocol reads a brand: only `typeFields.brands` and
`recordTypes.brands` do, and its slice keeps neither. So the emitter now
writes a type's own constructor without the brand when the slice of the
emission keeps no reader and the output reads no type's global other than
with `new` (where a second brand would throw), emitting the body again as it
does for namespace registration (docs/advanced-optimization.md, "Unread brands"), and the slice
drops `TypeBrand`.

The pellet's acceptance command, on the 24-CPU Mac (Node v24.18.0, macOS
15.7.3), baseline and candidate bench binaries alternated, two runs each, with
a peer worker building and testing (one-minute load 27 falling to 8 over the
four runs):

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only record-protocol --output DIR
```

| Build | Node `advanced` (µs) | Upstream `:advanced` (µs) | light-cljs / upstream | QuickJS (µs) |
| --- | ---: | ---: | ---: | ---: |
| master `fb3b6e4` | 34.4-34.8 | 19 | 1.9× | 1,036-1,038 |
| light-cljs-262 | 20.5-21.5 | 19 | 1.1× | 557-564 |

Sizes at `advanced` (`cargo nextest run --test compiler size_ratchet::
--no-capture`): the three fixtures that construct types without reading a
brand shrink by 176 bytes, the constructors' brand calls and the `TypeBrand`
unit, and their ceilings fall; every other fixture is unchanged, including
`eight-records`, which reads brands.

| Fixture | Advanced before | Advanced after | Advanced ceiling |
| --- | ---: | ---: | --- |
| record-protocol | 5,385 | 5,209 | 5,400 → 5,300 |
| three-records | 5,380 | 5,204 | 5,400 → 5,300 |
| pure-defs | 5,294 | 5,118 | 5,300 → 5,200 |

A program that updates or prints records through the generic operations
keeps `recordTypes.brands`, and with it the brand and its cost in Maglev;
cheaper brands for those programs are left to a follow-up.

## Runtime slices per chunk — October 18, 2026

light-cljs-279 slices the runtime per chunk at `advanced`: each runtime unit
goes to the chunk of the roots whose code needs it, and each chunk's runtime
is a function of the bindings it takes from the chunks before it
([code-splitting design](../docs/code-splitting.md), Phase 3, "As built
(light-cljs-279)"). Measured with `light-cljs --compile --optimize advanced
--size-report --entry app.main --source-root examples/features --out-dir
OUT`, and raw / `gzip -9 -n` bytes of each file, against master `ebeb14e`,
which the pellet was rebased onto (first measured against `3abc7ad`, where
startup was 9,046 bytes too). Every single file, and the chunks at `none`
and `prepare`, are byte for byte as master's. Apple M3 Ultra virtual machine
of the [environment](results/environment.txt), macOS 15.7.3, one-minute load
average 5.8 (sizes do not depend on it). examples/features at `advanced`,
terse:

| File | Before | After |
| --- | ---: | ---: |
| `app.main.js`, startup | 69,528 / 21,361 | 9,046 / 3,373 |
| `app.editor+app.reports.js` | 632 / 397 | 54,195 / 17,412 |
| `app.editor.js` | 361 / 258 | 5,453 / 2,459 |
| `app.reports.js` | 878 / 534 | 4,513 / 2,096 |
| all four | 71,399 / 22,550 | 73,207 / 25,340 |
| startup lower bound, the application without its features | 4,680 / 1,753 | 4,680 / 1,753 |

Startup's runtime is 64 units: the 38 the application without its features
keeps, the `Loadable` type, and 25 units (2,879 bytes) only the features
need, the hash-trie wrappers `lookup.trie` and `lookup.members` with the
lookup and hashing they require, which assign `lookup` and so stay in its
chunk (light-cljs-302 would make them hooks); the pellet closes with them
there by the user's decision. Loading the editor first costs 59,648 / 19,871
bytes and the reports first 58,708 / 19,508, against their upper bounds of
64,975 / 20,151 and 64,458 / 19,943 as their own entries (`bb
benchmarks/features.bb` on the same master); the second feature costs its
own file. The build grows by 1,808 bytes (2,790 gzipped, the runtime now
compressed in four files): the factories' parameter, argument, return and
destructuring lists were 1,573 / 983 bytes at `3abc7ad`, which module-level
units would save (light-cljs-303), and the variadic arity
of `str`, whose output-level cell no chunk reads but to call, is left out.

## Runtime units at module level in chunks — October 18, 2026

light-cljs-303 writes each chunk's runtime units at module level in terse
`advanced` output instead of a factory of the bindings it takes from earlier
chunks: one name table over the program's compiler names and the runtime's
bindings (a read `rt$x` and the binding `x` one name), the bindings later
chunks read exported, those a chunk's units and code take from earlier
chunks imported ([code-splitting design](../docs/code-splitting.md),
Phase 3, "As built (light-cljs-303)"). Measured as for light-cljs-279, raw /
`gzip -9 -n` bytes of each file, against master `20e88da`. Every single file,
the chunks at `none` and `prepare`, and readable (`pseudo_names`) chunks are
byte for byte as master's. Same machine; one-minute load average 13.6 (sizes
do not depend on it). examples/features at `advanced`, terse:

| File | Before | After |
| --- | ---: | ---: |
| `app.main.js`, startup | 9,046 / 3,373 | 8,820 / 3,235 |
| `app.editor+app.reports.js` | 54,195 / 17,412 | 53,646 / 17,027 |
| `app.editor.js` | 5,453 / 2,459 | 5,134 / 2,251 |
| `app.reports.js` | 4,513 / 2,096 | 4,291 / 1,939 |
| all four | 73,207 / 25,340 | 71,891 / 24,452 |

The build is 1,316 raw bytes (1.8%) and 888 gzip bytes (3.5%) smaller, of
the 1,573 / 983 the factories' lists and wrappers cost; the rest is what one
table costs over two (the names outside the lists, 154 bytes estimated) and
the import and export lines of the runtime bindings the factories' parameters
took. Loading the editor first now costs 58,780 / 19,278 bytes (59,648 /
19,871 before) and the reports first 57,937 / 18,966 (58,708 / 19,508).

## Browser macros in the playground — October 18, 2026

light-cljs-307 prototyped running the playground's macros in the browser's
own engine instead of the module's QuickJS ([docs/browser-macros.md](../docs/browser-macros.md),
which has the program and the method). A macro-heavy compile at `--optimize
prepare` (455 KB of output, near the 1,024-expansion limit), median of five
warm compiles in one worker, both engines alternating; output identical:

| Engine | Embedded QuickJS | Browser macros | Speedup |
|---|---|---|---|
| Node 24.18 (V8), direct | 490 ms | 197 ms | 2.5x |
| Chromium 153 (headless), worker | 468 ms | 152 ms | 3.1x |
| Firefox 155, worker | 570 ms | 224 ms | 2.5x |
| WebKit 26.6, worker | 454 ms | 143 ms | 3.2x |

The module grows from 6,385,991 to 6,444,001 bytes (2,018,361 to 2,040,034
gzip -9) while QuickJS stays linked. Without QuickJS it would lose about
1.05 MB raw and 0.39 MB gzipped (a QuickJS-only module built with the same
profile is 1,093,751 / 403,086 bytes).

## Kind tables for the seq operations — October 18, 2026

light-cljs-237 gives `seq`, `first`, `rest`, `next` and `count` one dispatch
point per call for lazy seqs and cells, and `first` one for the views of
vectors and arrays: the type's prototype holds a kind number under a private
symbol, and a wrapper of each operation calls the kind's branch from a
private table before the operation probes payload stores in turn
([compiler.md](../docs/compiler.md#lists-vectors-and-seqs),
[advanced-optimization.md](../docs/advanced-optimization.md#phase-2-kind-tables)).
It also makes a step of a seq cheaper: range and chunked cons cells are
class instances, list and cons cells are not frozen, a private store's `set`
assumes a new value, and where no protocol implementation exists a chunked
cons and `map`, `filter` and the indexed sequence functions read a runtime
chunk's store directly.

Command, on the 24-CPU Mac (Node v24.18.0, macOS 15 ARM64), "after" on this
change (one-minute load median 11.79, range 10.26–13.16) and "before" with
master `85207c9`'s runtime in the same tree (median 9.66, range 8.25–10.26),
while two other workers built and tested:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only seq-pipeline,record-protocol --output DIR
```

Median µs per call (upstream's in the same run after the slash):

| Workload | Engine | Level | Before | After |
| --- | --- | --- | ---: | ---: |
| seq-pipeline (n = 200) | Node | `advanced` | 292 / 78 (3.7×) | 149 / 78 (1.9×) |
| seq-pipeline | Node | `none` | 911 | 846 |
| seq-pipeline | Node | `prepare` | 926 | 852 |
| seq-pipeline | QuickJS | `advanced` | 2,744 | 1,742 |
| seq-pipeline | QuickJS | `none` | 9,574 | 9,351 |
| record-protocol (n = 2,000) | Node | `advanced` | 21.7 / 23 (0.96×) | 23.8 / 23 (1.1×) |
| record-protocol | Node | `none` | 296 | 302 |
| record-protocol | QuickJS | `advanced` | 581 | 665 |

`record-protocol` was already at upstream's time (light-cljs-258, 264 and
the record work since); its protocol call is a direct slot call that this
change does not touch, and its rows move within the run-to-run spread at
this load. When this change was first measured on master `97b538f`
(October 15, load 3.6), `seq-pipeline` went from 329 to 135 µs against
upstream's 80 / 71 (4.1× to 1.9×).

The per-operation microbenchmark the pellet asks for: `first`, `rest` and
`count` at each of the 1,000 positions of `(seq (vec (range 1000)))` (a
ChunkedSeq, positions from `(iterate rest s)`, so the program keeps the
wrappers), compiled at `advanced` and timed in Node, median of seven fresh
processes, each the median of 15 rounds of 200 passes after 3,000 warm
passes; two interleaved runs each at load 4–8:

| ns per call | `first` | `rest` | `count` |
| --- | ---: | ---: | ---: |
| before (`85207c9`) | 46.4 / 46.4 | 265.0 / 290.7 | 50.2 / 51.1 |
| after | 23.0 / 23.1 | 270.0 / 268.2 | 57.0 / 56.9 |

`first` of a view reads its branch (kind 3). `rest` is bound by allocating
the next ChunkedSeq and its public slots' `WeakMap` entry, not by dispatch.
`count` pays the wrapper's kind read and table 3's default branch before
the operation: a `count` branch for views (about 250 bytes) gave 56.4 ns,
since a ChunkedSeq counts its vector, which has no kind. The wrapper skips
the table for a value with no kind number (`value?.[kindOps]`), which gave `first` of
a view 51 ns instead of 54 before views had a kind.

What the steps were worth when first measured, on `seq-pipeline` with the
levels driver (ten untimed and twenty timed calls in a fresh Node, the median
of seven to eleven processes): the kind tables alone took it from 298 to
242 µs; not freezing a cons cell's payload 6%; `set` without its test for a
payload 8%; the branches of `count` 7%; a chunked cons reading its chunk's
store 14%; a wrapper per operation rather than one shared one 6%, and reading
the kind in each wrapper rather than in a shared function 10%. Range cells as
classes halved `(reduce + 0 (mapcat identity (partition 2 (range n))))`
(about 448 to 225 µs) but are not on `seq-pipeline`'s path.

**Sizes** (bytes at `advanced`, size ratchet; "before" master `85207c9`).
Programs that build a lazy seq or a cell keep the wrappers and branches of
the operations they call, 2,872 to 2,930 bytes (the user accepted about
2.8 KB for `seq-pipeline` going from 4.1× to 1.9×; the view kind adds 95);
others move by at most 66 bytes (`eight-records`). The runtime that every
`none` and `prepare` output embeds grew 10,452 bytes (`console-log-sum`
441,164 to 451,616), and those ceilings rose to the new sizes rounded up to
the next 1,000. The advanced ceilings that the growth passed rose to the new
sizes rounded up, for the pellet's speed target:

| Fixture | Before | After | Ceiling before | Ceiling after |
| --- | ---: | ---: | ---: | ---: |
| seq-pipeline | 26,110 | 29,040 | 27,000 | 30,000 |
| keyword-lookups | 31,227 | 34,132 | 32,000 | 35,000 |
| constant-literals | 22,290 | 25,162 | 23,000 | 26,000 |
| constant-keys | 43,793 | 46,671 | 44,000 | 47,000 |
| event-maps | 31,461 | 34,366 | 32,000 | 35,000 |
| mapv-count | 32,280 | 35,187 | 33,000 | 36,000 |
| closed-params | 14,000 | 14,003 | 14,000 | 15,000 |
| callbacks | 38,324 | 41,242 | 39,000 | 42,000 |
| protocol-params | 43,242 | 46,151 | 44,000 | 47,000 |
| features | 70,016 | 72,946 | 71,000 | 73,000 |
| features-direct | 69,624 | 72,554 | 70,000 | 73,000 |

examples/features as chunks (`chunks::terse_chunks_write_their_runtime_units_at_module_level`)
grows from 71,891 to 74,835 bytes, and that test's bound by the same 2,944.

## Map entries without the vector type — October 18, 2026

light-cljs-247 builds the map entries a map's host iteration yields without
the vector type where a program builds no vector, keeps sorted-map search
only where sorted maps are built, and lets structural equality probe the
seq kinds' stores, UUIDs and Vars only where a program builds them. Its
second commit makes the array map's public constructor a hook (as the hash
set's), keeps the types' statics only where a program names one or reads
an empty literal, and moves the set, transient and sorted-map branches of
`lookup` and the symbol branch of structural equality into wrappers their
builders install
([advanced-optimization.md](../docs/advanced-optimization.md#phase-2-builtin-types-map-entries)).
Bytes at `advanced` (gzip -9 after the slash where given), "before" master
`4716053`, "first commit" `66a18ea`:

| Program | Before | First commit | After |
| --- | ---: | ---: | ---: |
| `(js/console.log (if (js/Math.random) {:k :a} nil))` | 10,264 / 3,586 | 7,606 / 2,769 | 6,942 / 2,567 |
| `(js/console.log {})` | 8,979 | 6,342 | 5,784 |
| `(js/console.log [1 2 3])` | 5,246 | 4,636 | 4,526 |
| `(js/console.log #{1 2})` | | 4,496 | 4,248 |
| `(js/console.log (sorted-map :a 1))` | 38,381 | 38,199 | 37,088 |
| `(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}))` | 53,631 | 53,279 | 53,114 |

The pellet's target for the first program was 7,000 bytes; it is 6,942.

Size ratchet, at `advanced`; every other fixture is unchanged. The runtime
that `none` and `prepare` outputs embed grew 5,425 bytes over both commits
(`console-log-sum` 451,616 to 457,041): the lean definitions of map
entries, their members, the public constructors' hooks, the statics'
condition and the new hooks and wrappers, with comments. Those ceilings rose
to the new sizes rounded up to the next 1,000, and the advanced ceilings of
the fixtures that shrank by a step dropped to their new sizes rounded up:

| Fixture | Before | After | Ceiling before | Ceiling after |
| --- | ---: | ---: | ---: | ---: |
| map-only | 10,193 | 6,871 | 11,000 | 6,900 |
| map-printed | 14,651 | 12,529 | 15,000 | 13,000 |
| event-maps | 34,366 | 31,962 | 35,000 | 32,000 |
| keyword-lookups | 34,132 | 33,063 | 35,000 | 34,000 |
| constant-keys | 46,671 | 45,183 | 47,000 | 46,000 |
| constant-lookups | 4,242 | 3,328 | 4,300 | 3,400 |
| constant-literals | 25,089 | 24,993 | 26,000 | 25,000 |
| keyword-case | 3,862 | 2,948 | 3,900 | 3,000 |
| keyword-only | 3,789 | 2,875 | 3,800 | 2,900 |
| vector-only | 5,273 | 4,553 | 5,300 | 4,600 |
| vector-printed | 10,616 | 10,500 | 11,000 | 11,000 |
| set-only | 5,506 | 4,285 | 5,600 | 4,300 |
| atom-str | 2,745 | 2,518 | 2,800 | 2,600 |
| rest-fn | 10,455 | 10,379 | 11,000 | 11,000 |
| closed-params | 14,003 | 13,054 | 15,000 | 14,000 |
| eight-records | 32,242 | 31,933 | 33,000 | 32,000 |
| mapv-count | 35,129 | 34,999 | 36,000 | 35,000 |
| protocol-params | 46,094 | 45,879 | 47,000 | 46,000 |
| features | 72,946 | 72,633 | 73,000 | 73,000 |
| features-direct | 72,554 | 72,241 | 73,000 | 73,000 |
| seq-pipeline | 29,040 | 28,868 | 30,000 | 29,000 |
| callbacks | 41,242 | 40,857 | 42,000 | 41,000 |

Equality and lookup did not slow down. First commit: a Node loop over
20,000 `=` of keywords, vectors, lists and map entries, `get` on a hash map
and an array map, and 2,000 `hash` of lists, compiled at `advanced` with
both binaries and run twice each (load 6.4), gave the same times within 5%
except keyword `=` (1.42–1.48 to 1.22–1.25 ms) and list `hash` (0.86–1.00
to 0.76 ms), which got faster. Second commit, against the first: 200,000
`get` of an array map, a hash map, a set and a sorted map, 200,000 `=` of
symbols and 100,000 of array maps, at `none` and `advanced`, run twice each
(load 12–13), gave the same times within 5% (at `none`, `get` of a sorted
map 86.1–86.9 to 85.4–86.0 ms, of an array map 71.6–73.9 to 74.0–74.4 ms).

## Multimethods at Advanced — October 18, 2026

light-cljs-239: a multimethod keeps its tables as arrays, walks no hierarchy
and calls no `IMultiFn` dispatcher where the program could not tell. Bytes of
`light-cljs --compile --optimize advanced <file>` (raw / `gzip -9`), on master
`5695da8` and with this change; sizes are deterministic, and each program
prints the same at `none` and at `advanced` on both.

| Program | Before | After |
| --- | ---: | ---: |
| `(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}))` | 53,114 / 16,103 | 9,192 / 3,388 |
| the same with a `:default` method and three calls | 53,302 / 16,183 | 9,380 / 3,470 |
| `(js/console.log (if (js/Math.random) {:k :a} nil))` | 6,942 / 2,591 | 6,942 / 2,591 |
| `(derive ::b ::a) (defmulti m identity) (defmethod m ::a [_] 1) (js/console.log (m ::b))` | 54,094 / 16,386 | 45,410 / 13,992 |
| `(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (count (methods m)))` | 52,974 / 16,069 | 37,781 / 12,096 |

The pellet's program is under the 10,000 bytes the user accepted in place of
the 8,000 target; the multimethod adds 2,250 bytes to its map argument
(`MultiFn` 493, `structuralEqualPair` 361, `bestMethod` 320, `getMethod` 283,
`isaPlain` 198, the arrays, `addMethodDirect` and the printing of numbers in
messages). A million calls of a four-method multimethod with keyword dispatch
values at `advanced` take 267–269 ms against 418–445 ms before (Node, two
runs each, alternating, load 5.4–8.0): the arrays' scan of four methods is
cheaper than the persistent cache's lookup.

Size ratchet (`cargo nextest run --test compiler size_ratchet --no-capture`):
no `advanced` size moves except the new fixture `defmulti` (ceiling 9,200).
Every `none` and `prepare` output embeds the whole runtime, comments included,
which grew 5,458 bytes (457,041 to 462,834 for `(js/console.log (+ 1 2))`,
112,803 to 114,499 gzipped): both representations of the tables, the hooks
and their comments; those ceilings rose to the new sizes rounded up to the
next 1,000.

## Multimethods that move toward their readers — October 18, 2026

light-cljs-300 lets `Inert` accept a `defmulti` with a keyword, `fn` literal,
core function or function Var dispatch, a literal default and the global
hierarchy, so at `advanced` an unread one without methods is removed and a
chunked build moves one toward its readers
([code-splitting design](../docs/code-splitting.md), "Open questions",
"Multimethods and motion"). Measured with `light-cljs --compile --optimize
advanced --entry app.main --source-root tests/fixtures/chunks/definitions
--out-dir OUT`, raw / `gzip -9 -n` bytes of each file, against master
`b04e99a`; sizes are deterministic (one-minute load average 8.8). The
fixture's `app.shared/render`, whose methods are in the editor and the
reports, moves from startup to their shared chunk with the runtime's
`MultiFn` unit and what it requires:

| File | Before | After |
| --- | ---: | ---: |
| `app.main.js`, startup | 9,450 / 3,678 | 1,522 / 667 |
| `app.editor+app.reports.js` | 56,176 / 17,477 | 63,944 / 19,793 |
| `app.editor.js` | 8,720 / 3,751 | 8,716 / 3,753 |
| `app.reports.js` | 799 / 500 | 797 / 500 |
| all four | 75,145 / 25,406 | 74,979 / 24,713 |

`(defmulti m :k) (js/console.log 1)` at `advanced` is 151 / 140 bytes, as
`(js/console.log 1)` is, against 4,988 / 2,018 before. The size ratchet
moves nowhere: its one multimethod fixture, `defmulti`, has a method, and
examples/features has no multimethod; `none` and `prepare` output is
unchanged.

## Definitions that read a changeable core value stay pinned — October 18, 2026

light-cljs-363 keeps a chunked build from moving a definition whose
initializer reads a dynamic var or settable value (`*print-fn*`, row value
`any`) or a core value the program replaces, since a feature chunk would
evaluate it after startup changed the value
([code-splitting design](../docs/code-splitting.md), "Items and
signatures"). Such definitions stay removable when unread, so single-file
output does not grow. Measured with `light-cljs --compile --optimize
advanced`, raw / `gzip -9 -n` bytes, against master `a622a19` (sizes are
deterministic; one-minute load average 10.9):

| Program | Before | After |
| --- | ---: | ---: |
| `(def p *print-fn*) (js/console.log 1)` | 151 / 140 | 151 / 140 |
| `(def p inc) (set! cljs.core/inc dec) (js/console.log 1)` | 235 / 185 | 235 / 185 |
| `(defmulti m *print-fn*) (js/console.log 1)` | 5,507 / 2,235 | 151 / 140 |

The unread `defmulti` on `*print-fn*`, which light-cljs-300 kept as not
inert, is now removed with the runtime's `MultiFn`. Every size ratchet
entry and the four files of the `definitions` chunk fixture are byte for
byte unchanged; `none` and `prepare` output is unchanged.

## Private payloads for the remaining kinds — October 18, 2026

light-cljs-291 moves the payloads of the other values that an operation
builds one per call off `WeakMap` entries, as light-cljs-259 did for the seq
kinds: `reduced` markers (`reducedValues`), transients (`transients`), queues
(`queues`) and iterator seqs (`iteratorSeqs`) use `privateStore()`, and the
array maps and sets (`maps`, `mapKinds`, `sets`, `setKinds`, outside the hash
tries) a new `kindedStore()`, whose class adds the payload and the kind as two
private fields of one construction. A step of `repeat` stores no nil metadata
or hash, and `Cons`, `SymbolValue` and `constructorSlots` no null hash; the
compiler form bridge reads a repeat as having nil metadata, as it does a lazy
seq. Building any of these writes no `WeakMap` entry now (24 writes for the
values tests/support/js_runtime.test.js builds before, 0 after).

Four steady-state workloads join `--levels` for the kinds no workload built
per call: `reduced-early` (a `reduce` that a `reduced` ends, per iteration),
`transient-batches` (a transient vector and a transient array map per
iteration), `queue-ops` (`conj`, `peek` and `pop` on a queue) and
`iterator-seq` (`reduce` over `es6-iterator-seq` of an array's values).

Command:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript --samples 15 \
  --only seq-pipeline,keyword-lookups,event-maps,constant-literals,constant-keys,record-protocol,reduced-early,transient-batches,queue-ops,iterator-seq,map-assoc,set-conj,map-into,vector-into,vector-conj \
  --output DIR
```

Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM, 24 CPUs, 15 timed rounds,
two other workers running: "after" on this change (one-minute load 13.0 at
the start, 6.5 at the end), then "before" on master `8982972` with the new
workloads (6.5, then 4.5); upstream from the after run. Median µs per call:

| Workload | n | Engine | Level | Before | After | Upstream `:advanced` |
| --- | ---: | --- | --- | ---: | ---: | ---: |
| event-maps | 5000 | Node | `advanced` | 4,492 (34×) | 724 (5.5×) | 132 |
| event-maps | 5000 | Node | `none` | 20,516 | 16,787 | |
| event-maps | 5000 | QuickJS | `advanced` | 9,081 | 10,754 | |
| event-maps | 5000 | QuickJS | `none` | 129,549 | 139,892 | |
| keyword-lookups | 2000 | Node | `advanced` | 3,524 (20×) | 1,583 (9.1×) | 173 |
| keyword-lookups | 2000 | Node | `none` | 13,619 | 10,222 | |
| keyword-lookups | 2000 | QuickJS | `advanced` | 18,756 | 20,904 | |
| keyword-lookups | 2000 | QuickJS | `none` | 85,184 | 96,407 | |
| transient-batches | 2000 | Node | `advanced` | 5,313 (27×) | 2,200 (11×) | 199 |
| transient-batches | 2000 | Node | `none` | 6,475 | 3,152 | |
| transient-batches | 2000 | QuickJS | `advanced` | 21,166 | 24,286 | |
| transient-batches | 2000 | QuickJS | `none` | 39,877 | 43,865 | |
| queue-ops | 2000 | Node | `advanced` | 4,010 (24×) | 2,699 (16×) | 166 |
| queue-ops | 2000 | Node | `none` | 8,586 | 6,819 | |
| queue-ops | 2000 | QuickJS | `advanced` | 18,668 | 21,593 | |
| queue-ops | 2000 | QuickJS | `none` | 60,609 | 66,162 | |
| iterator-seq | 2000 | Node | `advanced` | 2,163 (34×) | 504 (7.9×) | 64 |
| iterator-seq | 2000 | Node | `none` | 3,598 | 1,859 | |
| iterator-seq | 2000 | QuickJS | `advanced` | 5,838 | 6,600 | |
| iterator-seq | 2000 | QuickJS | `none` | 19,571 | 21,348 | |
| reduced-early | 5000 | Node | `advanced` | 1,587 (25×) | 1,152 (18×) | 64 |
| reduced-early | 5000 | Node | `none` | 3,224 | 2,730 | |
| reduced-early | 5000 | QuickJS | `advanced` | 21,395 | 22,768 | |
| reduced-early | 5000 | QuickJS | `none` | 56,869 | 58,548 | |
| constant-literals | 2000 | Node | `none` | 21,050 | 15,229 | |
| constant-literals | 2000 | QuickJS | `none` | 129,706 | 136,144 | |
| constant-keys | 200 | Node | `advanced` | 622 | 683 | 299 |
| constant-keys | 200 | QuickJS | `advanced` | 6,177 | 6,808 | |

`prepare` moved as `none` did, within 2%. Every moved kind is faster in
Node, at every level, so each one moved. In QuickJS each is 3% to 18%
slower, as light-cljs-259 found for the seq kinds: adding a private field
constructs a class, which costs QuickJS more than a `WeakMap` entry that its
reference counting frees at once (light-cljs-292 takes up QuickJS's seq
throughput). Within the same pair of runs, `seq-pipeline`, `record-protocol`,
`constant-literals` at `advanced`, and the collection workloads `vector-conj`,
`vector-into`, `map-assoc`, `map-into` and `set-conj` moved by -6% to +7%,
the after run starting under twice the load; the earlier pair (before at load
6–9, an intermediate build at 9–18) put each of them within 5% too, except
`map-into` at n = 20,000, 7% faster. `constant-keys` at `advanced`, whose
calls look up constant map and set keys in a map, is 6% to 10% slower in
both engines over the two pairs: a `get` of a map's or set's store is a
JavaScript function where it was a native `WeakMap` method.

Two layouts were measured for the maps and sets. Four `privateStore()`s, a
store for each of `maps`, `mapKinds`, `sets` and `setKinds`, add two private
fields, so two constructions, per map; one `kindedStore()` per kind was faster
in Node at `advanced` in three interleaved runs: `event-maps` by 13% to 15%
and `keyword-lookups` by 21% to 24%, and within 3% in QuickJS, for about 220
more bytes at `advanced`.

Sizes (bytes, size ratchet): the runtime that every `none` and `prepare`
output embeds grew 2,016 bytes (`console-log-sum` 462,834 to 464,850), and
those ceilings rose to the new sizes rounded up to the next 1,000. At
`advanced`, every output that tests a value against the map, set, transient,
queue or reduced stores, which most outputs that keep a collection
operation do, keeps `privateStore` and `kindedStore` and grows 484 to 535
bytes (`keyword-only` 2,875 to 3,396, `event-maps` 31,962 to 32,476,
`seq-pipeline` 28,868 to 29,388); outputs without one (`console-log-sum`,
`add`, `multi-arity`, `record-protocol`) are unchanged. Eighteen advanced
ceilings rose to the new sizes rounded up to the next 100 below 10,000 bytes
and the next 1,000 above: for `event-maps` 6.2×, `transient-batches` 2.4× and
`iterator-seq` 4.3× as fast in Node at `advanced`, the growth light-cljs-259 accepted for
its seq kinds (up to 1 KB per ceiling).
For the same reason the bounds in tests/compiler/builtin_types.rs rose by 500
bytes (a map literal 6,942 to 7,453 bytes; `(count (.split "a,b" ","))` 1,876 to
2,361; protocol tests of a type or record 6,127 and 5,847 to 6,622 and 6,372; a
type's `count` 7,558 to 8,043), and that of the chunked build in
tests/compiler/chunks.rs by 600.

## Engines of --eval and --repl — October 18, 2026

light-cljs-305 runs the forms of `--eval` and `--repl` in V8 by default and
keeps QuickJS as `--engine quickjs`; the macros of both stay in QuickJS. The
release binary, each workload run alternately on both engines with
`--timeout-ms 120000` and an unlimited poll budget, the median of n runs.
Wall time includes process start; RSS is the process's maximum resident set
(`/usr/bin/time -l`). Load average 3 to 4 on a 24-vCPU Mac shared with two
other workers.

| Workload | n | QuickJS | V8 |
| --- | --- | --- | --- |
| Startup: `--eval --text 1` | 7 | 34.9 ms, 13.2 MB | 15.9 ms, 22.5 MB |
| Library loading: require `clojure.string`, `.set`, `.walk`, `.edn`, `cljs.reader` | 7 | 149.6 ms, 28.0 MB | 130.4 ms, 41.3 MB |
| Compute: `(reduce + (map inc (range 300000)))` | 5 | 5,125.8 ms, 27.1 MB | 373.2 ms, 47.8 MB |
| Promise jobs: a chain of 50,000 `.then` | 5 | 68.1 ms, 44.3 MB | 24.5 ms, 34.6 MB |
| Long session: `--repl` with 6,000 forms (3,000 `def`s of a 50-element vector, each summed) | 3 | 9,630.4 ms, 38.5 MB | 3,010.5 ms, 98.3 MB |

The long session first reached V8's 64 MB default heap limit at about
5,000 forms with 2.6 MB live: V8 keeps every source `eval` compiles in its
eval cache until a low-memory collection, and a session compiles a new
source per form. The V8 session now begins each operation with such a
collection when the heap holds more than half its limit; the 6,000 forms
end at 17.0 MB used before the final collection and 3.3 MB after
(`LIGHT_V8_STATS`). The long session is quadratic on both engines: each
`def`'s unit registers the namespace with every binding so far (13.8 MB of
unit source at 2,000 forms against 3.5 MB at 1,000); light-cljs-366 made it
linear (next section).

## Long REPL sessions — October 18, 2026

light-cljs-366 registers with the runtime only the definitions a session's
unit adds to a namespace whose reader context the runtime already has
(`registerReaderNamespace(name, {bindings: [...]}, true)`), instead of the
namespace with every definition so far, so the unit of a `def` no longer
grows with the session. The workload of the section above: `--repl` with N
forms, N/2 `(def vI (vec (range 50)))` each followed by
`(reduce + (map inc vI))`, release binaries of master `421ab68` (before) and
of this change (after), `--timeout-ms 120000 --poll-budget 1000000000000`,
wall time and maximum RSS from `/usr/bin/time -l`. The 6,000-form rows are
the median of 3 runs alternating before and after (load average 5.6 to 6.9
on a 24-vCPU Mac shared with two other workers); the other rows are one
run each (load 11 to 17). Unit source is the `v8-source` total of
`LIGHT_V8_STATS`; both binaries print the same 6,000 results.

| Forms | QuickJS before | QuickJS after | V8 before | V8 after | Unit source before | after |
| --- | --- | --- | --- | --- | --- | --- |
| 1,500 | 1.71 s, 17.7 MB | 1.58 s, 17.6 MB | 0.39 s, 72.3 MB | 0.28 s, 66.3 MB | 7.8 MB | 0.67 MB |
| 3,000 | 3.77 s, 24.2 MB | 3.07 s, 23.2 MB | 1.04 s, 95.7 MB | 0.64 s, 76.3 MB | 31.6 MB | 1.35 MB |
| 6,000 | 8.72 s, 38.5 MB | 5.82 s, 35.3 MB | 3.05 s, 106.8 MB | 0.85 s, 74.0 MB | 134.3 MB | 2.70 MB |

Unit source is now linear in the forms (450 bytes a form) where it was
quadratic, and so are both engines' wall times: about 1 ms a form on
QuickJS and 0.14 ms on V8 at every size, where doubling the session had
cost 2.2 to 3.3 times the time before. The session's analysis was already
linear (light-cljs-348): `EvalSession` compiles through a `DevSession`, so
it shares the incremental snapshot and touched-only registrations.

## WeakMap stores in QuickJS — October 18, 2026

light-cljs-292 gives QuickJS back the throughput that private-field stores
cost it (light-cljs-259, light-cljs-291). The evaluator states
`globalThis.lightCljsWeakMapStores = true` before its QuickJS session loads a
runtime, and `privateStore()` and `kindedStore()` then return `WeakMap`s
(with `reset` and, for the kinded store, a `kinds` map), as unforgeable a
brand; Node, browsers and the V8 macro host state nothing and keep the
private fields. The runtime reads the statement once when it loads and never
sniffs the engine
([compiler.md](../docs/compiler.md#lists-vectors-and-seqs)). In a QuickJS
microbenchmark, stamping and reading one value costs about 210 ns through a
private-field store and 65 ns through a `WeakMap`: adding the field runs the
derived constructor, the base constructor (which allocates a `this` it
discards) and the field initializer, where `WeakMap.prototype.set` is one
native call that reference counting cleans up. A derived base constructor,
which allocates nothing, saved only 20 ns of that.

Command, with a copy of the bench binary built on master `6280d29` and one
on this change, run in turn from the same tree (master then change, then
change then master), and once with master `b8c199a`'s bench binary, the
master before light-cljs-259, against this change:

```sh
cargo bench --locked --bench compiler -- --levels --samples 15 \
  --only seq-pipeline,reduced-early,transient-batches,queue-ops,iterator-seq,event-maps,keyword-lookups,constant-keys,constant-literals,record-protocol,vector-conj,map-assoc,set-conj,map-into,vector-into \
  --output DIR
```

Node v24.18.0, macOS 15 (Darwin 24.6) ARM64 VM, 24 CPUs, two other workers
running (one-minute load 8 to 10). Median µs per call, before / after, first
pair then second:

| Workload | n | Level | QuickJS | Node |
| --- | ---: | --- | ---: | ---: |
| seq-pipeline | 200 | `advanced` | 1,755 / 1,440; 1,757 / 1,446 | 152 / 151; 154 / 147 |
| seq-pipeline | 200 | `none` | 9,932 / 8,323; 9,884 / 8,328 | 887 / 904; 889 / 886 |
| reduced-early | 5000 | `advanced` | 22,515 / 17,034; 22,517 / 16,953 | 1,149 / 1,148; 1,141 / 1,151 |
| reduced-early | 5000 | `none` | 57,163 / 50,525; 56,915 / 50,489 | 2,749 / 2,749; 2,753 / 2,733 |
| transient-batches | 2000 | `advanced` | 24,622 / 21,002; 24,628 / 20,983 | 2,192 / 2,205; 2,154 / 2,171 |
| queue-ops | 2000 | `advanced` | 21,437 / 15,990; 21,436 / 15,990 | 2,723 / 2,703; 2,718 / 2,690 |
| iterator-seq | 2000 | `advanced` | 6,626 / 4,307; 6,677 / 4,257 | 491 / 499; 495 / 498 |
| event-maps | 5000 | `advanced` | 10,562 / 9,062; 10,548 / 9,048 | 721 / 719; 724 / 723 |
| event-maps | 5000 | `none` | 138,087 / 126,999; 138,746 / 126,023 | 16,766 / 16,894; 16,667 / 16,745 |
| keyword-lookups | 2000 | `advanced` | 20,942 / 18,871; 20,994 / 18,841 | 1,561 / 1,587; 1,579 / 1,593 |
| keyword-lookups | 2000 | `none` | 95,110 / 85,330; 94,770 / 85,128 | 10,130 / 10,135; 10,116 / 10,149 |
| constant-keys | 200 | `advanced` | 6,803 / 5,730; 6,854 / 5,724 | 672 / 663; 673 / 674 |
| constant-literals | 2000 | `none` | 137,007 / 121,064; 137,216 / 121,380 | 15,173 / 15,303; 15,088 / 15,059 |
| record-protocol | 2000 | `advanced` | 642 / 645; 644 / 652 | 24.8 / 23.9; 24.7 / 24.1 |

`prepare` moved as `none` did. Every QuickJS row that builds values of these
kinds is 10% to 36% faster, and each is now at or below light-cljs-291's
"before" (for example `queue-ops` 18,668, `iterator-seq` 5,838, `reduced-early`
21,395 and `constant-keys` 6,177 there). In Node every row, and the collection
workloads (`vector-conj`, `vector-into`, `map-assoc`, `map-into` and `set-conj`
at 2,000 and 20,000), stayed within 2% in the second pair and within 4% in the
first; light-cljs-291's Node gains stand.

Against the master before light-cljs-259 (`b8c199a`), `seq-pipeline` at
`advanced` in QuickJS takes 1,416 µs to its 2,593 (load 3), and 1,458 to its
2,719 in a second run (load 9 to 12); in Node 143 and 161 µs, against 330 after
light-cljs-259.

`constant-keys` in Node remains about 4% slower than before light-cljs-291
(555 to 575 µs in seven-process medians of the compiled program, after this
change): a CPU profile puts the difference in `structuralEqualPair`, whose
`maps.has` and `sets.has` now test a private brand on keywords, vectors, maps
and sets alike, where a `WeakMap`'s `has` did not depend on the receiver's
shape. Giving sets a `kindedStore` class of their own made it slower (598 µs).

Sizes (bytes, size ratchet): the runtime that every `none` and `prepare`
output embeds grew 387 bytes (`console-log-sum` 464,850 to 465,237), and the
nineteen ceiling pairs it passed rose to the next 1,000. At `advanced` no
output grew: the stores test a value that can hold a field with one shared
function (`canHoldField`, which V8 inlines) instead of an inline expression
in each method, which pays for the `WeakMap` branches. Outputs that keep
`kindedStore` shrink by 18 bytes (`keyword-only` 3,396 to 3,378), `defmulti` by 14, and those
that keep both stores by 53 to 61 (`seq-pipeline` 29,388 to 29,335, `atom-str`
3,013 to 2,952); the ceilings of `atom-str`, `vector-printed`, `map-printed`
and `defmulti` moved down.
## Per-method protocol dispatchers — October 18, 2026

light-cljs-265 gives each protocol method of fixed signatures that is not
`:extend-via-metadata` a dispatcher of its own at `advanced`: a function
literal that calls the receiver's slot and leaves every other call to the
runtime (docs/advanced-optimization.md, "Per-method dispatchers"). Before,
every dispatcher of the program was a closure of one function in
`protocolMethod`, so V8 kept one call feedback for all of them, and each
call allocated (`arities.filter`, rest arguments, `Array.from` and spread
calls). The shared path, which is every dispatcher below `advanced`, now
computes its arity tables and slots once per method and forwards
`arguments` as they are.

The new `protocol-dispatch` workload calls `area` and `perimeter` as values,
through a parameter of `total`, over 8 records of one type and 8 of four
types, 32 calls an iteration; the shapes are in arrays so that the calls
are what it times. The pellet's acceptance command, on the 24-CPU Mac (Node
v24.18.0, macOS 15.7.3), with two other workers building and testing:

```sh
cargo bench --locked --bench compiler -- --levels --upstream research/clojurescript \
  --only protocol-dispatch,record-protocol --output DIR
```

"Before" is this workload on master `52eb4f0`'s compiler. Milliseconds per
call `(f 2000)`, median / minimum; upstream is timed in the same rounds:

| Build | One-minute load (median of rounds) | Workload | light-cljs `advanced` | Upstream `:advanced` | light-cljs / upstream |
| --- | ---: | --- | ---: | ---: | ---: |
| before | 4.2 | protocol-dispatch | 7.921 / 7.258 | 0.202 / 0.198 | 39× |
| before | 4.2 | record-protocol | 0.022 / 0.020 | 0.021 / 0.019 | 1.0× |
| after | 9.0 | protocol-dispatch | 0.366 / 0.355 | 0.209 / 0.199 | 1.8× |
| after | 9.0 | record-protocol | 0.024 / 0.022 | 0.025 / 0.021 | 0.97× |

That is 124 ns a protocol call before and 5.7 ns after, against 3.3 ns
upstream. record-protocol's call is devirtualized at `advanced`
(light-cljs-262) and its output has no dispatcher, so this change leaves
its `advanced` output as it was; the difference is the load. The other
columns of the same runs, ms per call:

| Workload | Node none / prepare, before | Node none / prepare, after | QuickJS advanced, before / after |
| --- | ---: | ---: | ---: |
| protocol-dispatch | 10.20 / 10.48 | 4.40 / 4.42 | 70.1 / 11.2 |
| record-protocol | 0.295 / 0.296 | 0.110 / 0.106 | 0.628 / 0.641 |

Below `advanced` record-protocol runs the shared path, which no longer
allocates.

Two other shapes of the dispatcher were timed by hand on the way, in the
workload's output edited by hand (200 untimed and 30 timed calls in one
Node process). Reading the slot
once and calling the value, `const m2 = r0?.[m1]; return m2 != null ?
m2.call(r0, r0) : m0(r0)`, took 0.47 ms a call of the workload against
0.37 for the shape kept, which reads the slot again to call it, as
upstream's protocol functions do: the call through the property is what
lets V8 inline the four types' implementations. Setting the dispatcher's
`length` with `Object.defineProperties` took it to 0.73 ms, since `total`
then reads `call` from functions of another shape; a default parameter
value keeps the `length` at 1 instead. A first version of the workload
summed the shapes with `(reduce (fn [t s] (+ t (f s))) 0 shapes)` over
vectors: 13.9 ms before and 6.1 ms after (on master `7931693`), against
0.61 upstream. Most of what is left there is `reduce` and `seq` over a
vector of 8 records, about 0.7 µs each, in the core protocol wrappers
(`reduce.protocol`, `seq.protocol`) that a program with `installMethods`
keeps (light-cljs-302 and light-cljs-358).

**Bytes.** Each method's dispatcher is about 53 bytes of terse output,
`(z,A)=>function(B){return B?.[A]!=null?B[A](B):z(B)}`. What one more
protocol of two methods, implemented by the one record and both methods
called as values, adds at `advanced`, as
`size_ratchet::each_two_method_protocol_costs_at_most_its_budget` measures
it; upstream compiled the same programs:

| Per extra | Before | After | Upstream `:advanced` |
| --- | ---: | ---: | ---: |
| two-method protocol | 225 | 331 | 487–488 |

The runtime that `none` and `prepare` embed grew by about 560 bytes, so 18
of their ratchet ceilings rise by 1,000. Of the `advanced` fixtures only
`eight-records`, whose `area` calls run the dispatcher, changed: from
32,364 to 32,601 bytes. In the others no dispatcher is emitted or every
call is devirtualized.
