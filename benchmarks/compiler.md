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
