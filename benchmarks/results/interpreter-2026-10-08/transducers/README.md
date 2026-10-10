# Native transducer measurements, October 8–9, 2026

Nine same-binary pairs validated full equal outputs, including vector partition
groups and the last nine-item group, before measurement. The retained native xf
was reused with fresh per-process reducer state on every call. This run favors
transducers in eight workloads, but **rejecting every input is slower and
allocates more**. The partition adapters also request more total bytes despite
fewer allocation calls. These are specific native interpreter workloads, not a general
transducer, SCI/JVM or ClojureScript performance claim.

## Machine, sources and boundaries

Apple M3 Ultra virtual machine, 24 visible CPUs, 32 GiB RAM, macOS 15.7.3 arm64;
Rust 1.99.0 (`b940084d7`), LLVM 23.1.2, Cargo 1.99.0. Default release optimization,
no build overrides, affinity or frequency control. [Environment](environment.json)
records exact UTC capture time and dirty Git state. Nine raw samples × 32
operations per resident phase; two complete warmup batches per phase; one GC
per batch. Fixture and phase order is fixed. Builds and verification finished
before timing. Both sides share the same binary, retained finite vector input
and retained callables; the scalar pair constructs fresh unbounded ranges on
both sides. Neither side retains memoized transformed output between calls.

[Methodology and commands](../../../interpreter.md#equivalent-native-transducer-pipelines)
describe each boundary. Timed results pass through black_box; the checksum is
the scalar or outer vector length. Full recursive equality, including partition
vector shape, is checked outside timing. Parse, analysis and fixture setup are
excluded from resident calls. `construct_invoke_xf` includes xf creation;
`invoke_xf` and `eval_xf` reuse it, and still apply it afresh on every operation.

The [manifest](manifest.json) hashes exact sources, all fixtures, saved binaries
and the source archive. Sources and binaries are retained outside `target` at:

```
/Users/admin/.cache/light-cljs-baselines/light-cljs-58/transducers
```

The archive includes all accumulated uncommitted implementation work; HEAD
alone does not identify it. This report was written after measurements. No
production code or dependency was changed for this benchmark.

## Complete-pipeline invocation

Time is µs/operation: median ± MAD (min–max). MAD/ranges are observed variation,
not confidence intervals. Each operation returns and drops its scalar/vector.

| Fixture | Reused xf | Equivalent lazy pipeline | Median change |
| --- | ---: | ---: | ---: |
| `scalar` | 2396.42 ± 44.61 (2297.46–2448.46) | 3910.21 ± 62.61 (3784.10–4114.08) | -38.7% |
| `identity` | 751.89 ± 8.24 (725.65–763.59) | 1185.77 ± 9.63 (1148.12–1293.45) | -36.6% |
| `reject_all` | 332.29 ± 4.15 (323.81–343.60) | 216.22 ± 4.17 (209.36–224.46) | +53.7% |
| `early_take` | 30.95 ± 0.63 (28.73–33.63) | 40.65 ± 0.21 (40.15–41.28) | -23.9% |
| `mapcat` | 1645.88 ± 17.90 (1604.91–1736.48) | 2938.63 ± 35.87 (2868.36–3120.56) | -44.0% |
| `nested_cat` | 10.14 ± 0.23 (9.88–10.61) | 15.02 ± 0.04 (14.92–15.73) | -32.5% |
| `partition_sum` | 678.65 ± 6.51 (669.20–705.49) | 1042.53 ± 3.74 (1035.82–1053.78) | -34.9% |
| `partition_vector` | 478.33 ± 4.62 (468.97–487.00) | 983.99 ± 6.48 (973.27–998.29) | -51.4% |
| `custom_volatile` | 1290.53 ± 4.52 (1281.14–1297.21) | 1525.79 ± 15.35 (1509.80–1558.52) | -15.4% |

The scalar example yields 1001000; doubling mapcat yields 999000; partition sum
yields 500500. Materializing `identity`, `early_take`, `nested_cat`,
`partition_vector` and `custom_volatile` uses three-argument `into []` versus
two-argument `into` over equivalent lazy transformations. The custom xf uses an
interpreted volatile-backed counter created when applied, and matches lazy
`map-indexed`. Full values are checked on three successive uses before timing.

## Separate allocator instrumentation

Each cell is xf / lazy, per invocation. Allocations exclude reallocations;
requested bytes include each full new realloc size. These are allocator layout
requests for the benchmark process, not RSS, allocator reserved pages or
interpreter allocation-limit units. Instrumented timings are zero and are not
used in the timing table. Raw samples are in [allocations.csv](allocations.csv).

| Fixture | Allocations | Reallocations | Requested bytes |
| --- | ---: | ---: | ---: |
| `scalar` | 30,030 / 56,019 | 0 / 2,000 | 2,201,800 / 3,837,116 |
| `identity` | 10,955 / 19,953 | 1,015 / 2,015 | 1,986,048 / 2,533,860 |
| `reject_all` | 6,021 / 3,020 | 17 / 17 | 483,752 / 243,684 |
| `early_take` | 241 / 467 | 32 / 48 | 162,424 / 175,884 |
| `mapcat` | 25,025 / 52,076 | 17 / 1,019 | 1,788,000 / 3,339,044 |
| `nested_cat` | 186 / 291 | 5 / 7 | 12,168 / 17,548 |
| `partition_sum` | 9,480 / 15,257 | 1,208 / 144 | 1,802,280 / 1,065,356 |
| `partition_vector` | 8,287 / 15,416 | 1,017 / 365 | 1,650,176 / 1,210,988 |
| `custom_volatile` | 12,956 / 22,955 | 1,016 / 2,015 | 1,954,328 / 2,829,996 |

Retained/live and peak requested bytes below are absolute whole-process counts
in the measured region, **not per-operation**. Inputs and harness/runtime
storage are intentionally retained. Phases share a context in fixed order, so
these live/peak and slot fields include earlier phases' retained arena capacity;
they are not isolated minimum-memory comparisons. Values are median KiB
(1024 bytes); raw
bytes and every sample/range are retained in the [summary](summary.json).

| Fixture | Live KiB xf / lazy | Peak KiB xf / lazy |
| --- | ---: | ---: |
| `scalar` | 57,909.2 / 115,745.5 | 57,909.9 / 115,746.4 |
| `identity` | 39,882.3 / 76,432.1 | 39,882.8 / 76,432.4 |
| `reject_all` | 6,612.7 / 7,089.2 | 6,613.2 / 7,089.7 |
| `early_take` | 1,729.7 / 2,445.9 | 1,752.5 / 2,462.4 |
| `mapcat` | 30,633.4 / 118,582.7 | 30,634.1 / 118,583.5 |
| `nested_cat` | 356.1 / 900.9 | 357.0 / 901.6 |
| `partition_sum` | 44,089.0 / 51,667.0 | 44,090.0 / 51,667.9 |
| `partition_vector` | 42,811.4 / 53,331.5 | 42,812.0 / 53,332.2 |
| `custom_volatile` | 45,932.0 / 81,755.3 | 45,932.6 / 81,755.7 |

The successful materializing/reducing xf paths avoid intermediate lazy output
nodes, while retaining ordinary callback and persistent destination costs.
The all-rejecting case materializes no output on either side, yet the adapter
still receives every input; it made 6021 versus 3020 allocation requests here.
This counterexample is retained without optimization. Partition sum requested
1,802,280 versus 1,065,356 bytes (+69.2%); materialized partition vectors requested
1,650,176 versus 1,210,988 bytes (+36.3%). Fewer allocator calls therefore do not
imply fewer requested bytes. Native partitioning builds its persistent vector
buffer incrementally; the lazy path uses its existing partition representation
and converts groups for the materialized comparison. These aggregate counts
do not isolate every allocation site or establish a causal speedup model.

## Construction, application and analyzed calls

Median µs/operation. Construction invokes a retained factory. Application only
applies the retained xf to a retained completing reducer and drops the fresh
reducer; it does not traverse input. Both analyzed call phases evaluate already
analyzed invocations, excluding reading, analysis and input setup. Full MAD and
range distributions for these phases are in [summary.json](summary.json).

| Fixture | Construct xf | Apply xf | Construct + invoke | Analyzed xf | Analyzed lazy |
| --- | ---: | ---: | ---: | ---: | ---: |
| `scalar` | 0.70 | 0.65 | 2316.92 | 2355.95 | 3830.05 |
| `identity` | 0.32 | 0.17 | 754.22 | 738.96 | 1162.86 |
| `reject_all` | 0.45 | 0.17 | 329.37 | 337.29 | 225.53 |
| `early_take` | 0.55 | 0.50 | 30.87 | 28.08 | 38.14 |
| `mapcat` | 0.45 | 0.56 | 1634.04 | 1659.14 | 2946.00 |
| `nested_cat` | 0.27 | 0.45 | 10.63 | 9.98 | 14.98 |
| `partition_sum` | 0.35 | 0.26 | 673.59 | 677.10 | 1038.88 |
| `partition_vector` | 0.34 | 0.30 | 487.72 | 483.11 | 983.57 |
| `custom_volatile` | 0.16 | 0.35 | 1303.15 | 1308.83 | 1512.31 |

The end-to-end native CLI scalar path measured **5.518 ms** median,
MAD 0.202 ms, range 5.316–6.693 ms (nine processes).
It includes startup, file read, context setup, parse/analyze/evaluate, result
printing, pipe drain and exit. OS file caches are uncontrolled. All nine printed
1001000; see [fresh-process.csv](fresh-process.csv).

## Collection and retained memory

Every phase, including early-stopped, partial-buffer failure and cat-cursor
failure invocations, returned to identical warmed heap/name/source/code/
namespace/Var/root/slot counts after **every** sample. Failure cases construct
fresh lazy inputs and verify the expected ExceptionInfo category; errors and
outputs are dropped before collection. Successful scalar early termination
also constructs a fresh unbounded input on each call. Retained callable/input
roots are intentional. No extra per-run code/source/state accumulated.

GC cost below is median µs **per collection of a 32-operation batch**, outside
execution timing. Amortized GC cost per operation is this value divided by 32.
`H/S/C/R/T` means live heap values / sources / executable units / roots / slots.
Slots include retained vacancies and may differ with the high-water workload.

| Fixture | GC µs xf / lazy | Warm H/S/C/R/T xf | Warm H/S/C/R/T lazy |
| --- | ---: | ---: | ---: |
| `scalar` | 25485.21 / 52527.21 | 22 / 3 / 3 / 13 / 192441 | 22 / 3 / 3 / 13 / 480345 |
| `identity` | 9390.92 / 19215.71 | 16 / 3 / 3 / 14 / 32235 | 16 / 3 / 3 / 14 / 160203 |
| `reject_all` | 3384.46 / 1070.79 | 17 / 3 / 3 / 14 / 6283 | 17 / 3 / 3 / 14 / 32236 |
| `early_take` | 119.42 / 278.88 | 20 / 3 / 3 / 14 / 3829 | 20 / 3 / 3 / 14 / 4371 |
| `mapcat` | 14940.92 / 46745.67 | 18 / 3 / 3 / 14 / 64305 | 18 / 3 / 3 / 14 / 448529 |
| `nested_cat` | 95.46 / 190.17 | 24 / 3 / 3 / 14 / 723 | 24 / 3 / 3 / 14 / 2579 |
| `partition_sum` | 10770.75 / 15109.75 | 17 / 3 / 3 / 14 / 34389 | 17 / 3 / 3 / 14 / 168661 |
| `partition_vector` | 9809.62 / 14347.29 | 18 / 3 / 3 / 14 / 34387 | 18 / 3 / 3 / 14 / 170707 |
| `custom_volatile` | 14989.29 / 23051.00 | 21 / 3 / 3 / 14 / 32302 | 21 / 3 / 3 / 14 / 160270 |

Every new harness warmed GC row had **zero allocations, reallocations and
requested bytes**. The unchanged cyclic collector harness also passed ordinary
and instrumented runs at cadences 1, 16 and 256. It retained 317 heap values,
320 names, one source, one code unit, six namespaces, 314 Vars and six roots;
slots were 1432 / 2767 / 24127 respectively. Each instrumented GC sample had
zero requests. See [ordinary GC](interpreter_gc.log) and
[instrumented GC](interpreter_gc-allocations.log); use the ordinary file for
un-instrumented timing, not instrumented wall-clock numbers.

## Unchanged old-path regression scope

Adding the new fixtures correctly makes the general `run_interpreter.py` reject
the old complete fixture manifest. The [guard check](verification-identity-guard.log)
records that expected rejection. No identity rule was weakened. Instead, the
focused runner verifies the saved baseline source archive/binaries and every
original fixture, then links its **unchanged archived harness** to the current
library. Both sides use that exact old fixture set; [identity evidence](old-path-identity.json)
records the scope and hashes. Two rounds reverse binary order. Every old language
outcome matched in both rounds; the expanded current harness also passed all
its own expected outcomes in ordinary and instrumented builds.

Current versus saved baseline median change is shown below. Negative is faster.
The baseline predates bounded analysis/root reuse improvements, so these gains
cannot be attributed solely to the transducer additions.

| Old path | Round 1 | Round 2 | Allocation requests baseline / current |
| --- | ---: | ---: | ---: |
| `closures_loop/eval` | -14.1% | -12.0% | 6,666 / 514 |
| `closures_loop/read_analyze_eval` | -10.7% | -10.6% | 9,184 / 2,970 |
| `collections/eval` | +1.0% | +1.3% | 20,756 / 16,121 |
| `collections/read_analyze_eval` | +0.2% | +1.0% | 23,870 / 19,176 |
| `embedding/invoke` | -15.0% | -12.9% | 583 / 1 |
| `lazy/eval` | -3.1% | -0.8% | 10,658 / 7,571 |
| `lazy/read_analyze_eval` | -2.6% | -1.1% | 12,419 / 9,277 |
| `modules/cached_require` | -6.1% | +1.4% | 422 / 388 |
| `modules/first_load` | -9.8% | -3.6% | 58,942 / 47,094 |

Collections evaluation was 1.0% / 1.3% slower by median; observed sample ranges
overlap. Cached require changed sign between rounds (−6.1% / +1.4%). These
neutral/slower results remain in the report and all raw data. There is no
wall-clock CI threshold, universal speed claim or speculative optimization.

## Verification

- `cargo test --locked`: 485 passed, 10 opt-in tests ignored.
- `cargo test --locked --release`: 485 passed, 10 opt-in tests ignored.
- The complete native transducer suite: all 19 tests passed in both builds.
- `python3 tests/reference/sci/regenerate.py --checkout research/sci --native`:
  all 2386 pinned SCI cases matched; 356 native tests passed, one opt-in SCI test
  ignored because the explicit reference script already ran the oracle.
- Strict all-target Clippy passed both with and without `count-allocations`;
  formatting and whitespace checks passed.
- Nine-sample/32-operation new and existing harnesses passed in separate timing
  and allocation builds; ordinary/instrumented cyclic GC invariants passed.

[Debug](verification-debug.log), [release](verification-release.log),
[SCI/native](verification-sci.log), [Clippy](verification-clippy.log) and
[instrumented Clippy](verification-clippy-allocations.log) logs are retained.
Source/test semantics were not changed by the benchmark implementation.
