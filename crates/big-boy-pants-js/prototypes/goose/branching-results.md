# Bounded Goose branching comparison

**No-go for a compiler speedup at this measured scope.** The complete two-batch
compiler experiment at `0c39f9efe0ae97a4d1593ef228de73a726da61ed` takes
54.24 ms [53.53,57.42] for the one-worker V8 build on the larger case. The
Goose prototype takes 170.78 ms [161.78,171.54] at one worker, and its best
branching result is 136.51 ms [135.24,139.83] at four workers: 1.25 times its
own one-worker speed, but still 2.52 times V8's wall time. The one-worker
small/medium/larger deficits are 1.70/2.52/3.15 times V8. These results describe
the finite inputs below, with actual compiler/helper work, and do not predict
whole-project performance or justify an engine rewrite.

The source-derived VM experiment also loses. Its larger two-batch wall is
190.02 ms at one Goose worker, 165.43 ms at four workers (1.15 times its own
speed, still 1.63 times V8), 156.57 ms without branching, and 312.41 ms when
rebuilding a runtime for each job. Sharing setup helps; branching does not
beat the retained serial control in this envelope.

## Reproduce and evidence

From the repository root:

```sh
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --check
GOOSE_BENCH_DEBUG=1 bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --check
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --measure
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --allocations
bb crates/big-boy-pants-js/prototypes/goose/benchmark.bb --summarize
```

- [Clean timing samples](branch-samples.edn.gz): **1,050 samples, 210 cells,
  exactly five rounds each**, 14 quiet-machine gates and five separate copy
  probes. The manifest records source revision `0c39f9ef`, an empty tracked diff
  at invocation start, exact source/input/executable hashes and native commands.
- [All-cell medians, ranges, ratios and phase summaries](branch-summary.edn).
- [Separate allocation/storage samples](branch-allocations.edn.gz): 210
  instrumented cells, allocator requests, process RSS, native storage/copy
  counters and sampled thread counts. These are not clean timing samples.

Gzip contains newline-delimited EDN without rounding. Reproduction overwrites
its corresponding current artifact; `--check` writes a separate hidden file.
The compiler workload and output checks were strengthened for this revision;
all current cells were rerun. The Rust allocation and timing executables differ;
the same native executable selects instrumentation with an explicit runtime
flag. Both manifests record their executable hashes.

Historical evidence remains available without being substituted for this run:
[one-batch compiler timing samples](branch-samples-one-compiler-batch.edn.gz)
at `9e5da298b3781893a447a072922343b067bb1f36`, and
[their allocation samples](branch-allocations-one-compiler-batch.edn.gz) with
`11dacc2a`'s allocation-only output fix. That experiment measured compiler
branch cells with one application batch; its serial/rebuild and reused-batch
controls covered source-derived VM calls only. Its form check compared value
equality, form topology and source provenance but omitted the serialized
scalar datum pools; it did not establish exact scalar metadata or numeric
representation equality. Current checks compare those complete pools as well,
and regression tests reject both integer-versus-double and scalar metadata
changes that the older checks admitted. The old timing values remain historical
measurements under those weaker checks, not new correctness evidence.

The [earlier complete timing run](branch-samples-before-barrier.edn.gz) also
remains historical: its copy probe lacked the opaque consumption barrier and
is excluded from copy conclusions. The existing [serial evidence](serial-costs.md)
and [samples](serial-samples.edn), including 738/739's revisions and paired
copy-removal diagnostics, remain unchanged. Methodology follows
`7149c2a9:benchmarks/README.md` and `db6f7f07:benchmarks/parser.md`, not their
historical performance targets.

## Finite supported inputs and checks

Seed 24301; no adaptive selection. The actual emitted `work` macro adapts
`corpus/churn.cljs` / `benchmarks/native-macros-k.bb`: it persistently constructs
a vector and map, traverses them, and returns a quoted vector containing the
computed total and its original form argument. Calls alternate work `n` and
`n+1`. Numeric payload element `i` is `(24301+17*i) mod 101`.

| Case | Work n | Jobs per batch | VM source bytes | Payload | Payload nodes per call |
| --- | ---: | ---: | ---: | --- | ---: |
| small | 8 | 1 | 308 | vector of 8 numbers | 9 |
| medium | 32 | 4 | 705 | vector of 32 numbers | 33 |
| larger | 100 | 8 | 2705 | vector of 100 numbers | 101 |
| short | 32 | 4 | 2321 | vector of 100 two-unit strings | 101 |
| deep | 32 | 4 | 417 | 12 nested vectors around 7 | 13 |
| plain | 32 | 4 | 1345 | vector of 32 plain café strings | 33 |
| decoded | 32 | 4 | 1857 | same strings using a Unicode escape | 33 |

Plain and decoded have the same 128 UTF-16 payload units per call despite
different source byte counts. Manifests record exact source hashes, bytes,
payload nodes/units and compiler source bytes for both normal and rebuild
controls. Node counts above describe payloads, not all parser/metadata nodes.

The compiler reads actual `macros.cljs`, `batch0.cljs`, `batch1.cljs` and
`main.cljs`. Each batch contains the listed jobs; batch1 requires batch0, and
the application joins both sets of results. The macro demands the actual shared
helper `[11 22]` after collection construction. Each ordinary compilation uses
one adapter session and asserts one initialization, exactly twice the listed
job count, and exactly one helper producer. The second batch uses that same
session, emitted macro and helper publication. No fresh-compilation/session
lifetime guard is bypassed and no macro result is precomputed.

Each compiler sample checks byte-identical emitted JS against a separate V8
reference for its control, then executes it with Node and checks application
output. Rebuild emits one isolated application per source job; its result lines
are normalized and independently required to equal the ordinary application's
complete result vector. VM samples freshly capture V8 expected forms and the
selected engine's emitted factory, then check all returned form nodes,
metadata/origins, scalar datum/UTF-16 pools and source bytes/spans after releasing
job handles. Unsupported inputs fail; no comparison cell is skipped.

## Intervals, controls and threads

Every cell has one complete discarded process warmup, followed by five measured
rounds. Cell order rotates and reverses on alternate rounds. Every cell process
is fresh; filesystem caches are warm, with no cold-disk or JIT-saturation claim.

Compiler cold/reused measurements are **batches within one compilation**.
A test-only expansion observer marks completion of the last `work` expansion
in each dependent namespace. The cold interval starts before host/session setup
and includes preparation up to the first batch's completion. The reused interval
runs from that boundary through the second batch's completion. Source discovery
may prepare both namespaces during the cold interval; namespace settling and
other compiler work between expansions remain in these intervals. They are
observed compiler intervals, not isolated VM timers or separate compilations.
Emission, final writing and disposal after the second boundary remain in the
primary process wall.

The compiler serial controls use one slot. V8 and Pants use their ordinary
serial compiler paths, so these are intentional repeats of their one-slot
ordinary path. Goose retains one heap and runs synchronously, with no snapshot
capture/restore or execution threads. It still asks the real compiler for the
shared helper and evaluates that emitted helper in Goose. This finite pure
helper needs no nested macro job while its source is being prepared; the serial
measurement control does not admit that additional behavior. The normal
branching adapter's nested/reentrant behavior is unchanged and remains tested.

The compiler rebuild control creates and disposes a **fresh whole compiler
host and runtime for every source job**, in each of two batches. All engines
perform the same macro inputs and helper demand. Thus it deliberately repeats
source reading, macro preparation and compiler work as well as engine setup;
its difference from reuse is not a pure copying cost. The benchmark process
persists, including process-wide V8 platform and immutable engine/bootstrap
caches. There are no cached expansions or reused compilation sessions in this
control. Each fresh Goose compilation asserts its own one helper producer.

VM batch 0 is the first execution batch and batch 1 reuses the same runtime.
VM serial retains one evaluator/heap, while VM rebuild creates a fresh runtime
(and a fresh Goose subprocess) per job. These scopes differ from the compiler's
whole-source-compilation rebuild control.

Primary wall surrounds process launch through exit: source load/parse,
macro compilation/emission/export, runtime/process load and boot, capture,
transport copies, restore/relocation, queueing, execution, helper publication,
artifact/form conversion, JS writing, handle release, worker joins, session
file removal and final disposal. CLI argument parsing and Node application
execution are outside. Benchmark input generation, independent oracle setup,
external file comparisons and final benchmark-directory deletion are outside.
VM primary wall additionally includes its fresh independent reference/source
capture compilations. It is a checked source-derived VM experiment, not a
compiler speedup measurement. VM batch clocks exclude subsequent comparisons
and owned-result disposal; primary wall includes them.

Export/import/worker sums overlap compiler analysis and coordinator sleep;
they must not be added and called wall time. Compiler rebuild summaries sum
sequential per-compilation phase counters, retaining each raw session row.
Serial native invocation clocks include synchronous helper-source wait/load;
branch execution chunks stop at helper demand. Those counters are not identical
CPU-time scopes across controls.

One optimized Rust test executable contains all three selections. V8 build and
Pants build name the corresponding current MacroHost/MacroEvaluator paths here;
standalone feature-only CLI binaries are outside this comparison. V8 executes
canonical JS runtime calls; Pants and Goose execute emitted native form
intrinsics with their respective native leaves. Inputs and outcomes match;
internal work differs.

V8 has one execution worker, an auto-sized background pool
(`new_default_platform(0,false)`, `--no-concurrent-osr`) and an interrupt
scheduler. Compiler slots include the coordinator; Pants uses N-1 additional
compiler threads for N slots. VM Pants uses N explicit execution threads plus
a waiting coordinator. Branching Goose uses one native coordinator, one input
thread and at most N execution threads across all Rust submitters, plus two
Rust transport-reader threads and the compiler/VM submitters. Serial Goose uses
one native thread for input, coordination and execution. Rebuild jobs are
sequential with a one-worker limit. There is no N-by-N execution pool. Reference
preparation/compiler syntax bootstrap can leave V8 background threads in other
selections. Instrumented thread polling records lower bounds, not exact peaks.

## Setup-inclusive results

Milliseconds: median [minimum,maximum], five samples per cell. All seven shapes,
all controls and phase ranges are in `branch-summary.edn`. Both scopes below
perform two batches; the compiler helper is included only in compiler rows.

| Build / execution limit | Compiler small | Compiler medium | Compiler larger | VM larger |
| --- | ---: | ---: | ---: | ---: |
| V8 build / 1 | 34.99 [34.77,36.64] | 41.86 [40.37,42.89] | 54.24 [53.53,57.42] | 101.51 [95.82,103.82] |
| Pants build / 1 | 82.57 [81.27,84.10] | 86.19 [83.74,89.07] | 93.85 [92.46,96.37] | 142.84 [142.60,143.51] |
| Pants build / 2 | 83.28 [80.80,89.22] | 86.09 [83.57,87.47] | 92.00 [91.13,94.63] | 139.00 [138.07,144.77] |
| Pants build / 4 | 82.17 [81.70,87.65] | 85.48 [83.81,88.09] | 90.10 [88.74,93.93] | 140.98 [136.18,142.62] |
| Pants build / 8 | 83.08 [81.52,85.46] | 87.51 [86.99,90.84] | 91.95 [91.27,92.12] | 143.49 [142.42,156.43] |
| Goose prototype / 1 | 59.64 [57.15,60.44] | 105.69 [105.43,107.95] | 170.78 [161.78,171.54] | 190.02 [187.85,195.10] |
| Goose prototype / 2 | 83.78 [81.08,86.25] | 131.53 [128.87,133.36] | 191.71 [189.69,196.78] | 168.57 [164.67,169.09] |
| Goose prototype / 4 | 109.11 [104.04,112.67] | 104.42 [100.73,107.52] | 136.51 [135.24,139.83] | 165.43 [165.15,168.65] |
| Goose prototype / 8 | 108.42 [105.63,109.84] | 117.42 [102.64,121.87] | 137.83 [132.99,145.95] | 168.57 [166.04,170.57] |

Larger one-worker controls below separate cold and reused batch medians. Full
walls include all final cleanup. Batch spreads are in the summary. Rebuild's
second batch remains rebuild-per-job; it is not a reused runtime.

| Scope / build / control | First batch ms | Second batch ms | Complete process ms [min,max] |
| --- | ---: | ---: | ---: |
| Compiler / V8 / ordinary | 38.78 | 6.70 | 54.24 [53.53,57.42] |
| Compiler / V8 / serial | 38.41 | 6.61 | 53.37 [52.47,54.81] |
| Compiler / V8 / rebuild source job | 151.90 | 133.49 | 292.97 [284.70,299.43] |
| Compiler / Pants / ordinary | 78.24 | 5.43 | 93.85 [92.46,96.37] |
| Compiler / Pants / serial | 80.04 | 5.53 | 96.49 [93.68,98.54] |
| Compiler / Pants / rebuild source job | 136.87 | 69.89 | 214.97 [213.43,221.06] |
| Compiler / Goose / branch | 94.21 | 62.20 | 170.78 [161.78,171.54] |
| Compiler / Goose / serial no branch | 46.23 | 13.59 | 69.77 [66.80,74.19] |
| Compiler / Goose / rebuild source job | 264.03 | 253.10 | 524.94 [521.37,533.98] |
| VM / V8 / ordinary serial | 11.50 | 5.22 | 101.51 [95.82,103.82] |
| VM / V8 / rebuild runtime | 97.19 | 99.22 | 268.31 [256.82,273.31] |
| VM / Pants / branch | 11.61 | 8.32 | 142.84 [142.60,143.51] |
| VM / Pants / serial no branch | 3.82 | 3.50 | 139.49 [135.34,143.96] |
| VM / Pants / rebuild runtime | 38.80 | 38.92 | 206.19 [198.72,213.53] |
| VM / Goose / branch | 28.83 | 28.65 | 190.02 [187.85,195.10] |
| VM / Goose / serial no branch | 13.54 | 12.56 | 156.57 [154.36,161.59] |
| VM / Goose / rebuild runtime | 93.75 | 94.81 | 312.41 [312.00,318.11] |

There is no observed copying break-even at which branching wins over the
retained serial control or V8. Larger compiler Goose saves setup relative to
524.94 ms of source-job rebuilds, but its serial retained heap is faster still
at 69.77 ms; that remains 1.29 times V8's ordinary wall. These controls change
compiler preparation, heap lifetime, dispatch and synchronization as well as
copying. Their differences cannot be attributed entirely to byte copies.
Plain-versus-decoded shapes do not establish a per-form or decoding advantage;
all measured ranges, bytes and nodes remain available in the raw manifest.

## Attribution, copying and storage

For larger one-worker compiler Goose, the 170.78 ms primary wall contains
155.81 ms inclusive analysis, 0.36 ms emission, 1.94 ms host/session setup and
4.42 ms compiler/session cleanup. Rust GSP export sums to 11.70 ms, form bridge
to 2.88 ms, native restore to 2.89 ms and native execution chunks to 7.99 ms.
These overlap analysis. The native coordinator sleeps 151.08 ms over a
155.02 ms observed interval; compiler preparation, callbacks, transport and
workers overlap that sleep. The worker-active measurements leave an upper
bound of 90% idle or uninstrumented capacity at one worker and 97% at four;
they are not exact CPU-utilization measurements. Four workers reduce inclusive
analysis to 123.54 ms, while the complete wall is still 136.51 ms. Raw copying
primitives do not remove the dominant compiler/demand/dispatch path.

For larger one-worker VM Goose, the primary wall includes 116.06 ms of
reference/source preparation, 2.41 ms runtime setup, 28.83/28.65 ms batches and
4.48 ms final disposal. Native execution sums to 5.03 ms and restore to 0.42 ms;
the coordinator sleeps 58.15 ms over 58.77 ms observed wall. This explicitly
bounds how much faster copying alone could affect the setup-inclusive result.

Copy probes use an opaque non-inlined C consumption barrier for every nonempty
payload component. Five fresh probes separately measure raw flat copies,
barrier-only consumption, semantic validation, capture and validated
restore/relocation. Gross copy time includes its barrier; it is not silently
subtracted. The initialized base is 5,338 bytes including its record header
and 5,002 payload bytes, so this cache-resident primitive cannot predict a
large live continuation's cost. Storage results below come from the separate
instrumented invocation, not the clean wall-time cells.

The separate run's 1,000 raw base copies take 104 us [95,106], with a
30 us [29,31] barrier-only control. Validation takes 3 us [2,3], capture
5 us [5,6], and validated restore/relocation 17 us [12,18]. Validation visits
198 records using an 8-byte bitmap. Capture accounts for 5,338 bytes including
the header; restore copies 5,002 payload bytes. These different inclusive
scopes are not subtracted to invent exclusive relocation time.

Larger one-worker VM branching restores 80,032 payload bytes, transfers
296,736 structured form bytes in both directions, and peaks at 776,417 logical
private-heap bytes. The larger two-batch actual compiler restores 12,765,554
bytes across worker/continuation stages, with snapshot sizes 5,338–799,752
bytes. Its shared helper publishes 188 bytes once, versus 3,008 bytes across
16 fresh per-job compilations in the rebuild control. This separates copying
initialized base data, retaining collection-heavy continuations, and transferring
new immutable artifacts. The serial compiler has zero snapshot/restore bytes,
but retains 12,418,038 logical heap bytes across the session; its append-only
heap and source pools also increase structured bridge traffic to 647,004 bytes
versus 334,992 for branching. Retention has a measurable storage cost.

Larger one-worker accounting below comes only from the separate instrumented
run. Requested bytes are cumulative Rust allocator requests, not live memory.
RSS is decimal MB; Rust and native Goose peaks are independent and are not
added as if measured simultaneously. Rebuild's native column is the largest
individual child-process peak, not their sum.

| Scope / build / control | Rust allocations | Reallocations | Requested bytes | Rust RSS MB | Goose RSS MB |
| --- | ---: | ---: | ---: | ---: | ---: |
| VM / V8 / ordinary | 235,501 | 33,715 | 53,781,130 | 69.32 | n/a |
| VM / Pants / branch | 752,794 | 158,981 | 150,327,308 | 85.18 | n/a |
| VM / Goose / branch | 636,571 | 154,140 | 133,339,666 | 77.30 | 3.92 |
| Compiler / V8 / ordinary | 182,603 | 25,607 | 46,661,277 | 61.10 | n/a |
| Compiler / Pants / ordinary | 473,596 | 125,730 | 98,057,221 | 49.74 | n/a |
| Compiler / Goose / branch | 204,053 | 34,112 | 52,393,734 | 41.96 | 22.87 |
| Compiler / Goose / serial | 200,218 | 33,003 | 53,054,996 | 40.63 | 15.14 |
| Compiler / Goose / rebuild | 285,226 | 45,300 | 196,495,588 | 74.02 | 7.63 |

Rust counts include complete preparation, checking, profiling output and Rust/
Pants allocations; they exclude V8 C++ malloc and Goose native data-stack
allocations. Instrumented V8 diagnostics and final forced GC are absent from
clean timing. The VM macro context reports 2,995,168 used bytes and 6,815,744
heap-capacity bytes before that GC; compiler V8 reports 4,011,572 used and
8,126,464 heap-capacity bytes. VM Pants reports 5,998,058 managed bytes,
20,743,126 peak-managed bytes and 37,501,014 cumulative admitted bytes. These
accounting values differ from allocator requests, RSS and flat retained payloads.
The one-worker VM process observations see 19/20/22 Rust threads for V8/Pants/
Goose, including background services. Short native worker lifetimes can evade
polling; thread observations remain lower bounds.

Snapshot counters count logical capture/restore bytes, not every transport
copy or continuation-stack/message frame. Fresh dispatch copies the flat Saved
packet into spawn and worker-owned storage; resumes/messages add transfers.
Complete wall/RSS includes those costs. Published helper payload bytes are
separate from initialized base and continuation copying; bridge bytes describe
structured forms, not all source-image traffic. Goose array capacity is not
exposed and is not inferred from payload or RSS. No flat-ID-versus-tree layout
comparison or scratch-capacity-reuse ablation is claimed.

## Machine and verification

Apple M3 Ultra **Virtual**, 24 visible CPUs, 32 GiB RAM; macOS 15.7.3 (24G419),
ARM64. Goose v2026.3-267-g369c7a2c/runtime 3025e5810379, Babashka 1.12.218,
Rust 1.99.1 (58cf8f9c3)/LLVM 23.1.3, V8 crate 152.2.0; Node v26.12.1 supplies
application-output checks. Goose native compilation uses `/usr/bin/cc -O2`
(Apple clang 21.0.0), `-lm -pthread`, without debug checks in timing. Cargo uses
release optimization, thin LTO and one codegen unit. No custom RUSTFLAGS,
CPU affinity or native-CPU override. Build parallelism is three, outside timing.
The clean run's 14 gates require at least 90% overall idle and no competing
Cargo/Goose build or measurement. Recorded idle is 93.90–98.20%; manifests and
quiet rows retain process/load observations. These are VM-host measurements
without CPU-frequency control or a confidence-interval claim.

The current implementation passed all 1,050 clean timing comparisons and 210
separate allocation/storage cells. All 210 cells also passed with both ordinary
and debug native runners; the scalar metadata/representation negative regression
passed. `compiler.bb` passed 15 adapter tests in each of debug/native modes,
including concurrent helper production, nested callbacks, failures and cleanup;
Goose format/check/debug/native smoke passed. Changed Rust is formatted and
`git diff --check` passes.

Strict Clippy still stops at the same eight existing engine deprecated-atomic,
constant-chunk and byte-string diagnostics. The focused
`--features goose-macros --lib --tests` audit, also repeated with
`count-allocations`, passes with only those baseline categories and the existing
namespace test's single-element loop permitted. The pinned research checkout
was linked only for those checks and removed afterward. No production Rust
compiler/adapter implementation, dependency or build configuration changed in
this revision; broader default/QuickJS suite and all-target lint results from
`124c6910` are reused on that basis. They retain the documented limits: default
library 367 passed; feature-enabled default library 543 passed with two matching
baseline failures (namespace publication requires a native engine; process-order
capture metadata), QuickJS 498 passed with the matching namespace failure,
QuickJS bridge 3/3 passed, baseline-allowed all-target Clippy passed. Root
rustfmt has existing unrelated differences, and the full compiler suite's
existing wasm dead-code blocker remains; neither is represented as passing.
