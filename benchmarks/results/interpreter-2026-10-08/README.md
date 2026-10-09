# Native release baseline, October 8, 2026

The [method and reproduction commands](../../interpreter.md) define every
measurement boundary. This is an initial Rust baseline, not a performance
optimization or a comparison against SCI/JVM. The environment is an Apple M3
Ultra virtual machine, 24 CPUs, 32 GiB RAM, macOS 15.7.3, Rust/Cargo 1.99.0,
LLVM 23.1.2, aarch64-apple-darwin, standard release optimization, no build flags.
The [environment](environment.json) records the exact invocation and dirty Git
state. [Manifest hashes](manifest.json) identify actual sources/fixtures/binaries.

## Timings

Nine samples, 32 operations per resident batch. Values are median ± median
absolute deviation in **microseconds per operation**. MAD is descriptive
variation, not a confidence interval. Min/max and every phase/GC distribution
are in [summary.json](summary.json); all original durations and checksums are in
[interpreter.csv](interpreter.csv).

| Resident phase | Closure/loop | Collections | Lazy pipeline |
| --- | ---: | ---: | ---: |
| Parse | 1.93 ± 0.07 | 1.98 ± 0.07 | 1.10 ± 0.05 |
| Lower pre-parsed root | 9.04 ± 0.30 | 9.72 ± 0.45 | 5.22 ± 0.12 |
| Pull read (parse + lower) | 10.96 ± 0.27 | 11.38 ± 0.45 | 6.74 ± 0.47 |
| Analyze, including macros | 118.88 ± 1.65 | 152.10 ± 1.51 | 80.96 ± 1.94 |
| Reuse analyzed code | 383.74 ± 2.78 | 929.29 ± 5.85 | 498.91 ± 8.01 |
| Read/analyze/eval | 518.41 ± 4.74 | 1110.59 ± 2.50 | 589.50 ± 7.43 |
| GC after 32 evaluations, amortized per eval | 220.04 | 572.18 | 324.18 |

The constructor/drop median is 0.52 µs; adding namespace registry initialization
is 2.49 µs. These do not eagerly install every builtin. A retained embedding
closure call (64-step loop) costs 34.70 ± 0.29 µs. First resident three-module
load/initialization/result costs 2821.04 ± 13.67 µs; cached require plus caller
read/analysis costs 22.49 ± 0.14 µs. File I/O and process creation are excluded
from these resident measurements.

Fresh native CLI invocation through printed result and exit includes filesystem
reads, context setup and teardown. Nine fresh processes per fixture, with OS
caches uncontrolled. Median ± MAD in **milliseconds**:

| Fixture | Fresh process |
| --- | ---: |
| Closure/loop | 3.452 ± 0.170 |
| Collections | 4.261 ± 0.281 |
| Lazy pipeline | 3.459 ± 0.139 |
| Three-module report | 6.809 ± 0.333 |

[Fresh-process raw samples](fresh-process.csv) preserve every result.
The [repeat comparison](comparison.json) reran saved binaries alongside the
final binaries: ordinary interpreter non-GC phase medians ranged from −7.7%
to +5.2%, and CLI medians from −4.1% to +2.7%. Short phases are especially
noisy. `repeat-*.csv` contains that saved-binary run, not reconstructed samples.
No optimization was attempted, so these differences are not claimed speedups.

## Allocations and observed bottlenecks

Counts come from a **separate instrumented binary**. Times in its raw CSV are
zero. Each byte figure below is the median allocation layout total; it is not
RSS. Before/after live bytes include retained interpreter capacity and harness
storage across the entire process. Heap counts are interpreter records.

| Warm operation | Allocations/op | Requested bytes/op | Live MiB before/after GC | Heap values before/after GC |
| --- | ---: | ---: | ---: | ---: |
| Closure/loop eval | 6666 | 266624 | 10.68 / 2.54 | 82 / 50 |
| Collection eval | 20756 | 1039400 | 39.97 / 19.56 | 90525 / 61 |
| Lazy eval | 10658 | 523284 | 19.38 / 9.78 | 61766 / 38 |
| Embedding invoke | 583 | 23320 | 1.00 / 0.29 | 10 / 10 |
| Cached require | 422 | 19852 | 1.47 / 1.31 | 534 / 310 |

See [allocations.csv](allocations.csv) for repeated counts, reallocations,
peak/live requested bytes and all record/slot counts. First module load requests
about 4.44 MB across about 58,942 allocations. Measured warmed collections
allocate no new objects, but still spend substantial time scanning/reclaiming
temporary records. Backing capacity remains after collection.

Execution is the largest measured resident phase on these fixtures. Analysis is
the next significant component; standalone syntax parsing is a small component.
GC adds roughly 220–572 µs per evaluation when collected every 32 operations,
so it must not be hidden when evaluating a speed change. The closure loop has
large allocation/collection costs even though only 50 heap values survive.
Code inspection shows `Owner::root` creates an Rc and weak registry entry for
each new rooted value, including scalars. Transient root creation and scanning
are concrete candidates for a follow-up experiment; this is not proof that
changing them will be safe or faster. The counters are not an allocation-site
or exclusive CPU profiler.

[Optional diagnostics](diagnostics.log) identify `report.metrics/with-base` at
`report/metrics.cljc:3:1`, its 27 generated semantic nodes, and source forms with
multiple code generations. Reused embedding calls generate no additional code.
These instrumented clocks are excluded from the ordinary timing summaries.
Multiple generated forms can share a source origin; repeated generation does
not establish equivalent output or permit caching macro effects.

## Frontend regression check

The existing lexer/parser harnesses were preserved and rerun with nine samples
targeting 100 ms each, including actual ClojureScript sources. A separate source
archive of Git HEAD `5434d0f3ad3cd9f15771eece4b83614ca9bb9851` was built with the
same toolchain as a frontend reference. Its
[identity](frontend-head-identity.json) and
[lexer](frontend-head-tokenizer.csv)/[parser](frontend-head-parser.csv) samples
are retained. This comparison spans the earlier native-reader parser changes
as well as this benchmark work; it does not attribute differences solely to
diagnostics. Positive percentages below mean slower ns/traversal.

| Input | Lexer vs HEAD | Parser vs HEAD |
| --- | ---: | ---: |
| Short atoms | +3.0% | **+15.0%** |
| Dense parentheses | +1.7% | +4.4% |
| Large reader | +0.9% | +5.5% |
| `cljs/core.cljs` | +0.5% | +4.9% |
| `cljs/analyzer.cljc` | +3.3% | +4.2% |

The parser short-atom slowdown remains visible: earlier current runs were
6.8% and 13.3% slower than the same HEAD reference. Smaller changes vary with
run order/VM scheduling; the final repeat control's parser medians range from
−6.3% to +2.7% without a frontend code change. Report this regression candidate
and investigate it rather than claiming universal unchanged performance.
[Full frontend comparison](frontend-comparison.json),
[current lexer](tokenizer.csv), and [current parser](parser.csv) contain all
inputs. No parser/tokenizer optimization is part of this change.

## Known-good handoff and validation

The final baseline is saved outside the repository/build directory:

```text
/Users/admin/.cache/light-cljs-baselines/light-cljs-44/verified-baseline
```

It contains immutable copies of the CLI, timing benchmarks and separate
allocation benchmark under `bin/`, plus exact source archive, hashes, raw
results and environment. The source archive was extracted, all archived hashes
verified, and interpreter/parser/tokenizer release benchmarks rebuilt from it.
Saved binaries remain usable after `cargo clean`. To measure a subsequent
change against this baseline:

```sh
python3 benchmarks/run_interpreter.py \
  --output "$HOME/.cache/light-cljs-baselines/next-comparison" \
  --compare "$HOME/.cache/light-cljs-baselines/light-cljs-44/verified-baseline" \
  --samples 9 --iterations 32 --frontend-ms 100 \
  --frontend-source research/clojurescript/src/main/cljs/cljs/core.cljs \
  --frontend-source research/clojurescript/src/main/clojure/cljs/analyzer.cljc
```

The final sources passed 410 tests in each of debug/release (10 explicitly
ignored external/tool checks), including the integrated 2,087-row native corpus,
and strict `cargo clippy --all-targets --all-features -- -D warnings`.
Diagnostic tests preserve results, output, effects, error classes, live records
and exact language allocation-budget boundaries. The benchmark driver checked
all fixture results and matched saved/current benchmark outcomes. No SCI/JVM
timing, arbitrary speedup goal or statistically significant speedup is claimed.

## Subsequent bounded-reuse comparison

The [bounded-reuse report](reuse/README.md) compares the subsequent scalar-root,
analysis-scratch and context-local analysis reuse changes against this preserved
baseline. It includes two cold/warm runs, exact 2,087-row baseline/current
observations, focused invalidation tests and final repository validation.
