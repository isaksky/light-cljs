# Bounded Goose branching comparison

**No-go for a compiler speedup from this prototype at the measured scope.** On
2026-11-03, the Goose prototype's single-worker compiler wall time was
1.40/1.91/2.23 times the V8 build for the small/medium/larger cases. Increasing
the execution limit to eight did not close the gap. The larger compiler case
was 45.06 ms [44.49, 45.67] for V8 and 100.46 ms [98.48, 105.23] for Goose at
one worker. This is a completed performance loss, not evidence about whole
projects or a reason to extend this experiment into an engine rewrite.

The source-derived VM experiment does benefit from sharing initialization:
Goose's larger fresh-process two-batch wall falls from 312.35 ms when rebuilding
per job to 190.85 ms with one initialized process, then 163.01 ms at two workers.
That is 1.17 times its single-worker branch speed, but still 1.70 times V8's
96.00 ms. Serial execution without branching takes 153.66 ms. Avoided setup and
fast primitives therefore do not establish viability.

## Reproduce and evidence

From the repository root:

```sh
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --check
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --measure
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --allocations
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --summarize
```

The final clean run is revision `9e5da298b3781893a447a072922343b067bb1f36`.
The storage run includes the subsequent allocation-only `--nocapture` fix at
`11dacc2a`; clean timing behavior is unchanged by that fix. Sources, executables,
input SHA-256s, native commands, toolchain and environment are in each manifest.
Documentation and the explanatory source comment follow those runs.

- [Raw clean samples](branch-samples.edn.gz): 840 samples, 168 cells, exactly
  five rounds per cell, plus 14 quiet snapshots and five separate copy probes.
- [All-cell medians, ranges, ratios and phase summaries](branch-summary.edn).
- [Separate allocation/storage samples](branch-allocations.edn.gz): one
  instrumented invocation per cell, with complete process RSS, sampled thread
  counts, allocator requests, retained-storage counters and native traces.
- [Earlier complete timing run](branch-samples-before-barrier.edn.gz): retained
  to expose repeat variation; its preliminary copy probe lacked the opaque
  consumption barrier and is excluded from copy conclusions. The primary tables
  use only the final run, not the best of two runs.

Gzip contains newline-delimited EDN without rounding. Re-running measurement
commands replaces their corresponding current samples; `--check` writes a
separate hidden correctness file. The existing [serial evidence](serial-costs.md)
and [serial samples](serial-samples.edn), including 738/739's revisions, scopes
and paired copy-removal diagnostics, remain unchanged. None supplies a new row
in this experiment. The methodology follows `7149c2a9:benchmarks/README.md` and
`db6f7f07:benchmarks/parser.md`, not their historical throughput targets.

## Finite supported inputs

Seed 24301; no random search or adaptive selection. The actual emitted `work`
macro adapts `corpus/churn.cljs` / `benchmarks/native-macros-k.bb`: it constructs
and persistently updates a vector and map, traverses them, and returns a quoted
vector containing the computed total and its original form argument. Calls
alternate work `n` and `n+1`. Numeric payload element `i` is `(24301+17*i) mod 101`.

| Case | Work n | Fan-out | VM source bytes | Payload | Payload nodes per call |
| --- | ---: | ---: | ---: | --- | ---: |
| small | 8 | 1 | 308 | vector of 8 numbers | 9 |
| medium | 32 | 4 | 705 | vector of 32 numbers | 33 |
| larger | 100 | 8 | 2705 | vector of 100 numbers | 101 |
| short | 32 | 4 | 2321 | vector of 100 two-unit strings | 101 |
| deep | 32 | 4 | 417 | 12 nested vectors around 7 | 13 |
| plain | 32 | 4 | 1345 | vector of 32 plain café strings | 33 |
| decoded | 32 | 4 | 1857 | same strings using a Unicode escape | 33 |

The last two have the same 128 UTF-16 payload units per call. Their source byte
counts differ. Manifests record exact source bytes, payload nodes/units, source
hashes and generation parameters; nodes here are payload nodes, not every parser
node or metadata occurrence. Wider work/fan-out and shape controls are bounded;
there is no arithmetic-only replacement or extrapolation beyond this manifest.

The compiler experiment loads actual `macros.cljs` and `main.cljs` namespaces,
uses compiler worker slots, analyzes returned forms and emits the application.
Its macro also demands the shared helper `[11 22]`, once, after constructing the
collections. Independent branches consume that immutable publication and join
in the application's result vector. Goose asserts one producer for every
compilation. Every compiler sample checks byte-identical emitted JS and actual
Node application output against a separate V8 reference. VM samples freshly
capture independent V8 expected forms and selected-engine emitted factories,
then compare complete returned forms, metadata/origins and source bytes/spans
after releasing job handles. Unsupported inputs fail; no cell is skipped.

## Intervals, controls and threads

Each cell has one complete discarded process warmup, then five measured rounds.
Cell order rotates and reverses on alternate rounds. Every process is fresh;
filesystem caches are warm and no JIT saturation or cold-disk claim is made.
VM batch 0 is the first execution batch and batch 1 reuses the same initialized
runtime/session. Rebuild controls deliberately create a fresh runtime/process
for each job in both batches. Serial controls retain one evaluator/heap without
per-job branches; V8's ordinary one-worker path is already serial.

Primary wall time surrounds process launch through exit. It includes source
load/parse, macro compilation/emission/export, runtime/process load and boot,
capture, transport copies, restore/relocation, queueing, execution, helper
publication/import, form conversion, emitted artifact writing, handle release,
worker joins, session-file removal and final process disposal. Compiler timing
uses the real namespace API; CLI argument parsing and application execution
are outside it. Benchmark input generation, independent compiler oracle setup,
result-file comparison and final benchmark-directory deletion are outside.
No precomputed expansion is used by a timed compiler invocation.

The VM primary wall additionally includes its two fresh reference/source-capture
compilations. It is a checked source-derived VM experiment, not a compiler
speedup measurement. Diagnostic batch clocks omit subsequent result comparison
and owned-result disposal; primary wall includes them. Export/import counters
and worker sums can overlap compiler analysis and coordinator sleep. They must
not be added together or presented as an exclusive wall-time decomposition.

One optimized test executable contains all three selections. V8 build and Pants
build denote the corresponding current MacroHost/MacroEvaluator path here;
standalone feature-only CLI binaries are outside this comparison. V8 executes
canonical JS runtime calls; Pants and Goose execute compiler-emitted form
intrinsics with their respective native leaves. Inputs and checked outcomes
match, while their internal work differs. Goose's initialized base is its bounded
runtime; each job adds its emitted factory and helpers publish once.

V8 has one macro execution worker, an auto-sized V8 background pool
(`new_default_platform(0,false)`, `--no-concurrent-osr`) and one interrupt
scheduler. Compiler slots include the coordinator, so Pants uses N-1 additional
compiler execution threads for N slots. Source-VM Pants uses N explicit execution
threads and one waiting coordinator. Goose uses one native coordinator, one
input thread and at most N execution threads, shared across all Rust submitters;
its Rust side also has two transport-reader threads and the compiler/source-VM
submitters. There is no N-by-N nested execution pool. Reference preparation and
compiler syntax bootstrap can leave V8 background threads in the other selections.
Instrumented `ps -M` observations are lower bounds from polling, not exact peaks
or a claim that all N workers were simultaneously busy.

## Setup-inclusive results

Milliseconds: median [minimum, maximum], five samples per cell. All seven cases,
all controls, phase ranges, ratios to one-worker V8 and speedups against each
engine's own one-worker branch appear in `branch-summary.edn`.

| Build / execution limit | Compiler small | Compiler medium | Compiler larger | VM larger, two batches |
| --- | ---: | ---: | ---: | ---: |
| V8 build / 1 | 35.13 [32.66,36.08] | 38.78 [38.03,40.42] | 45.06 [44.49,45.67] | 96.00 [93.91,106.47] |
| Pants build / 1 | 80.01 [78.31,89.89] | 81.34 [80.86,82.76] | 85.91 [85.20,90.85] | 141.99 [139.89,143.71] |
| Pants build / 2 | 81.88 [79.60,83.41] | 82.79 [81.49,84.58] | 87.08 [84.05,91.46] | 137.60 [136.31,148.89] |
| Pants build / 4 | 80.79 [79.43,83.41] | 82.61 [80.73,83.92] | 84.94 [83.53,87.66] | 137.90 [136.02,144.18] |
| Pants build / 8 | 80.45 [79.98,82.39] | 82.06 [81.48,85.25] | 88.55 [87.61,90.75] | 137.78 [137.11,141.28] |
| Goose prototype / 1 | 49.00 [47.44,51.04] | 74.13 [71.75,75.43] | 100.46 [98.48,105.23] | 190.85 [187.14,194.56] |
| Goose prototype / 2 | 73.16 [71.95,73.91] | 96.89 [96.01,97.58] | 129.60 [123.88,135.74] | 163.01 [159.17,167.01] |
| Goose prototype / 4 | 87.81 [82.92,89.27] | 87.33 [86.85,89.44] | 104.02 [101.02,106.10] | 163.19 [161.76,166.33] |
| Goose prototype / 8 | 88.18 [85.36,91.61] | 97.09 [91.32,99.33] | 107.86 [102.12,111.00] | 167.36 [165.41,172.20] |

Goose's larger compiler speedup at 2/4/8 workers is 0.78/0.97/0.93, so these are
slowdowns. Its best larger VM speedup is 1.17, with a remaining 1.70 times V8
deficit. The other shapes also lose; plain-versus-decoded differences overlap
run variation and do not demonstrate a decoding or per-form speed advantage.

The larger one-worker controls below separate first and reused batches. These
are diagnostic batch medians in ms; full walls and every batch range are in the
summary. Rebuild's second batch remains rebuild-per-job, not a reused session.

| Build / control | First batch | Second batch | Complete fresh process |
| --- | ---: | ---: | ---: |
| V8 / serial ordinary path | 11.52 | 5.16 | 96.00 |
| V8 / rebuild each job | 96.73 | 95.21 | 261.78 |
| Pants / branch | 11.70 | 8.28 | 141.99 |
| Pants / serial no branch | 3.72 | 3.42 | 137.17 |
| Pants / rebuild each job | 38.08 | 39.09 | 201.48 |
| Goose / branch | 29.30 | 29.16 | 190.85 |
| Goose / serial no branch | 12.31 | 11.95 | 153.66 |
| Goose / rebuild each job | 92.52 | 93.33 | 312.35 |

## Attribution, copying and storage

For larger Goose VM at one worker, clean medians are 114.31 ms reference/source
preparation, 2.23 ms runtime setup, 29.30/29.16 ms batches and 4.75 ms final
session disposal. Native execution totals 5.20 ms across both batches and
restore totals 0.40 ms. The native coordinator accumulates 59.35 ms asleep over
its 59.86 ms observed interval. Sleep overlaps worker execution and time awaiting
Rust submissions; it is not 59.35 ms of removable exclusive overhead. The
worker-active measurements leave 82% of one-worker capacity idle or unmeasured;
at eight workers the corresponding upper bound is 96%. These bounds include
unobserved queue/spawn activity and are not exact CPU-utilization measurements.

For the larger actual compiler at one worker, its 100.46 ms wall contains
88.24 ms inclusive analysis, 0.23 ms emission and 3.95 ms cleanup. Goose reports
5.89 ms Rust GSP export, 1.43 ms Rust form bridge, 1.47 ms native restore and
3.83 ms native execution/wait chunks. The coordinator sleeps 85.70 ms while
compiler preparation, callbacks, transport and workers overlap. Its append-only
continuations retain the collections built before the helper demand. The
complete compiler's setup/serialized demand/transport path dominates; raw copy
speed alone does not remove the observed deficit.

Copy probes use an opaque non-inlined C consumption barrier for every nonempty
payload component. An assembly check retained 14 call sites for that barrier. Five
fresh probes separately measure flat copies, barrier-only consumption, semantic
validation, capture and validated restore/relocation. Gross flat-copy time is
reported with its barrier control, not silently subtracted. The small initialized
base is 5,338 bytes including its record header and 5,002 payload bytes. This
cache-resident primitive cannot predict a larger live continuation's cost.

In the separate storage run, 1,000 base copies take 97 us [95,105],
with a 31 us [27,33] barrier-only control. Semantic validation takes
2 us [1,2], capture 6 us [5,6], and validated restore/relocation 15 us [12,18].
Validation visits 198 records and uses an 8-byte validation bitmap. These scopes
are inclusive and different; their medians are not subtracted to invent an
exclusive relocation time. No copying break-even is observed in the measured
work/fan-out envelope: sharing a process beats rebuilding, while removing both
branches and thread/coordinator hops beats branching. The controls change more
than a byte copy, so that difference is not attributed entirely to copying.

The larger two-batch VM restores exactly 80,032 payload bytes from a 5,338-byte
base, transfers 296,736 structured form bytes, and reaches 776,417 logical
private-heap bytes in a branch. The actual compiler restores 6,383,898 bytes
across its worker/continuation stages; snapshots range from 5,338 to 799,321
bytes. Its shared helper publishes only 188 bytes once. This large difference
between initialized base, retained private continuation and newly published
artifact is visible in the raw mode-tagged native traces.

The table below is the larger case at one execution worker. All values come
from the separate instrumented run. Requested bytes are cumulative Rust allocator
requests, not live bytes. RSS columns are decimal MB for the Rust process and
the native Goose child separately; their independent peaks are not a measured
simultaneous sum. The profile adds resource-wrapper processes and thread polling.

| Scope / build | Rust allocations | Reallocations | Requested bytes | Rust RSS MB | Goose RSS MB |
| --- | ---: | ---: | ---: | ---: | ---: |
| VM / V8 | 235,381 | 33,620 | 52,989,058 | 71.14 | n/a |
| VM / Pants | 752,685 | 158,884 | 149,535,468 | 82.38 | n/a |
| VM / Goose | 636,443 | 154,046 | 132,547,633 | 72.43 | 3.96 |
| Compiler / V8 | 160,282 | 23,252 | 42,708,769 | 55.69 | n/a |
| Compiler / Pants | 423,857 | 122,094 | 88,679,383 | 41.76 | n/a |
| Compiler / Goose | 171,040 | 27,525 | 45,585,442 | 41.58 | 16.66 |

Rust counts include the complete run, source/reference preparation, result
checks, profiling output and Rust/Pants allocations. They exclude V8 C++ malloc
and Goose native data-stack allocations. V8 diagnostic heap statistics and
forced final GC occur only in this invocation. The VM V8 macro context reports
3,022,264 used bytes and 6,815,744 heap-capacity bytes before that GC. The VM
Pants runtime reports 5,998,058 managed bytes, a 20,743,126-byte managed peak
and 37,501,014 cumulative admitted bytes; these accounting quantities differ
from RSS and allocator requested bytes. V8 and the other selections retain
background service threads: the VM one-worker samples observe 19/20/22 Rust
threads for V8/Pants/Goose. With the V8 harness waiter, execution thread and
interrupt scheduler accounted for, 16 remaining threads are background threads.
Native worker lifetimes can be shorter than the observer interval; sampled
thread counts are explicitly lower bounds.


Snapshot counters report logical bytes copied by capture/restore, not every
compiler/runtime transport copy or explicit continuation-stack/message framing.
Fresh dispatch also copies the flat Saved
packet into spawn storage and worker-owned storage; resumed continuations and
queue messages add transfers. Complete wall and process RSS include these costs.
Native published helper payload bytes are reported separately from initialized
base and continuation copying. Form wire bytes count structured bridge transport,
not all source-image or allocator traffic. Allocation requests, cumulative bytes,
logical retained bytes, capacity/accounting bytes and physical RSS are distinct.
Goose array capacity is not directly exposed and is not inferred from retained
payload bytes or RSS. No comparison of flat IDs against another tree layout, or
scratch-capacity-reuse ablation, is claimed.

## Machine and verification

Apple M3 Ultra **Virtual**, 24 visible CPUs, 32 GiB RAM; macOS 15.7.3 (24G419),
ARM64. Goose v2026.3-267-g369c7a2c/runtime 3025e5810379, Babashka 1.12.218,
Rust 1.99.1 (58cf8f9c3)/LLVM 23.1.3, V8 crate 152.2.0; Node v26.12.1 supplies
the separate application-output checks. Native Goose uses
`/usr/bin/cc -O2` (Apple clang 21.0.0), `-lm -pthread`, no debug checks; Cargo
uses release optimization, thin LTO and one codegen unit. No custom RUSTFLAGS,
CPU affinity or native-CPU override. Build parallelism is three, outside timing.
The clean run records 14 gates requiring at least 90% overall CPU idle and no
competing Cargo/Goose process. Recorded idle is 93.22–98.15%; before/after process/load
snapshots are in the manifest and quiet rows. These are VM measurements without
CPU-frequency control or a confidence-interval claim.

Verification on the measured implementation: all 840 timing samples across 168 cells and
168 storage invocations checked outputs; `compiler.bb` passed 15 tests in each
of debug/native modes; prototype format/check/debug/native smoke passed. The
default library suite passed 367 tests. Feature-enabled library suites reproduced
exact documented baseline failures: default 543 passed / 2 failed (native engine
required in namespace publication; process-order capture metadata), QuickJS
498 passed / 1 failed (same namespace test). QuickJS bridge tests passed 3/3.
Strict Clippy fails on the unchanged engine deprecated/chunk/byte-string
categories and namespace test's single-element loop; audits allowing only those
baseline categories pass for default, QuickJS and allocation-feature all targets.
Root rustfmt has the existing unrelated differences; changed Rust/Goose files
are formatted. The full compiler suite's existing wasm dead-code blocker is
unchanged and was not rerun. The pinned research checkout was linked only for
all-target checks and removed afterward.
